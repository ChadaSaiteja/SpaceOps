//! Tier 1 Direct NTFS MFT (Master File Table) scanner orchestrator.

pub mod record;
pub mod usn;
pub mod volume;

use crate::metadata::flags_from_raw;
use common::{
    CancellationToken, DirMetadata, FileMetadata, NodeId, ScanError, ScanEvent, ScanProgress,
    ScanSummary,
};
use record::{apply_fixups, parse_mft_record, ParsedMftRecord, MFT_RECORD_SIZE};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};
use volume::{get_volume_cluster_size, open_volume};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Ioctl::FSCTL_GET_NTFS_FILE_RECORD;
use windows::Win32::System::IO::DeviceIoControl;

pub const ROOT_MFT_RECORD: u64 = 5;

#[repr(C)]
struct NtfsFileRecordInputBuffer {
    file_reference_number: i64,
}

/// Read a single MFT record by its record number using FSCTL_GET_NTFS_FILE_RECORD.
pub fn read_mft_record_raw(
    volume_handle: HANDLE,
    record_number: u64,
    out_record: &mut [u8; MFT_RECORD_SIZE],
) -> io::Result<()> {
    let input = NtfsFileRecordInputBuffer {
        file_reference_number: record_number as i64,
    };

    // Buffer big enough for NTFS_FILE_RECORD_OUTPUT_BUFFER header (12 bytes) + 1024 bytes record
    let mut output_buf = [0u8; 12 + MFT_RECORD_SIZE];
    let mut bytes_returned = 0u32;

    let success = unsafe {
        DeviceIoControl(
            volume_handle,
            FSCTL_GET_NTFS_FILE_RECORD,
            Some(&input as *const _ as *const _),
            std::mem::size_of::<NtfsFileRecordInputBuffer>() as u32,
            Some(output_buf.as_mut_ptr() as *mut _),
            output_buf.len() as u32,
            Some(&mut bytes_returned),
            None,
        )
    };

    if success.is_err() {
        return Err(io::Error::last_os_error());
    }

    if (bytes_returned as usize) < 12 + MFT_RECORD_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Short MFT record read",
        ));
    }

    out_record.copy_from_slice(&output_buf[12..12 + MFT_RECORD_SIZE]);
    Ok(())
}

pub enum MftScanError {
    MidScanFailure(String),
    Fatal(ScanError),
}

impl From<ScanError> for MftScanError {
    fn from(e: ScanError) -> Self {
        MftScanError::Fatal(e)
    }
}

/// Execute a Tier 1 direct MFT volume scan on the given drive root.
/// Returns (Vec<ScanEvent>, ScanSummary, high_usn).
pub fn scan_mft(
    root: &Path,
    cancel: &CancellationToken,
    progress_interval: Duration,
    on_progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<(Vec<ScanEvent>, ScanSummary, i64), MftScanError> {
    let drive_letter = volume::extract_drive_letter(root)
        .ok_or_else(|| MftScanError::Fatal(ScanError::InvalidPath(root.to_path_buf())))?;

    if !volume::is_ntfs_volume(root) {
        return Err(MftScanError::MidScanFailure(
            "Volume is not formatted as NTFS".into(),
        ));
    }

    let volume_handle = open_volume(drive_letter).map_err(|e| {
        MftScanError::MidScanFailure(format!(
            "Failed to open raw volume \\\\.\\{drive_letter}:: {e}"
        ))
    })?;

    let cluster_size = get_volume_cluster_size(drive_letter)
        .map_err(|e| MftScanError::MidScanFailure(format!("Failed to query cluster size: {e}")))?;

    // Query USN journal to capture the baseline high USN
    let journal_info = usn::query_usn_journal(volume_handle.raw()).ok();
    let high_usn = journal_info.map(|j| j.next_usn).unwrap_or(0);

    let _start_time = Instant::now();
    let mut raw_record = [0u8; MFT_RECORD_SIZE];

    // In-memory indexing of parsed records: parent_record -> children list
    let mut parent_to_children: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut records: HashMap<u64, ParsedMftRecord> = HashMap::new();

    let mut record_num = ROOT_MFT_RECORD;
    let mut consecutive_failures = 0usize;
    let max_consecutive_failures = 100; // Stop when MFT ends
    let mut files_scanned = 0u64;
    let mut bytes_scanned = 0u64;
    let mut last_progress = Instant::now();

    while consecutive_failures < max_consecutive_failures {
        if cancel.is_cancelled() {
            return Err(MftScanError::Fatal(ScanError::Cancelled));
        }

        match read_mft_record_raw(volume_handle.raw(), record_num, &mut raw_record) {
            Ok(()) => {
                consecutive_failures = 0;
                if apply_fixups(&mut raw_record) {
                    if let Some(parsed) = parse_mft_record(&raw_record, record_num, cluster_size) {
                        if parsed.base_record_number == 0 {
                            if !parsed.is_directory {
                                files_scanned += 1;
                                bytes_scanned += parsed.allocated_size;
                            }
                            parent_to_children
                                .entry(parsed.parent_record_number)
                                .or_default()
                                .push(record_num);
                            records.insert(record_num, parsed);
                        }
                    }
                }
            }
            Err(_) => {
                consecutive_failures += 1;
            }
        }

        if last_progress.elapsed() >= progress_interval {
            last_progress = Instant::now();
            on_progress(ScanProgress {
                files_scanned,
                bytes_scanned,
                current_path: root.to_path_buf(),
            });
        }

        record_num += 1;
    }

    if records.is_empty() {
        return Err(MftScanError::MidScanFailure(
            "Zero records recovered from MFT stream".into(),
        ));
    }

    // Now emit topologically ordered ScanEvents starting from root (MFT record 5)
    let mut events = Vec::with_capacity(records.len() * 2);
    let root_node_id = NodeId(ROOT_MFT_RECORD);

    events.push(ScanEvent::EnteredDirectory {
        parent_id: None,
        id: root_node_id,
        meta: DirMetadata {
            name: root.as_os_str().to_os_string(),
            is_reparse_point: false,
        },
    });

    let (total_size, file_count, dir_count) = emit_mft_subtree(
        ROOT_MFT_RECORD,
        root_node_id,
        &records,
        &parent_to_children,
        &mut events,
    );

    events.push(ScanEvent::DirectoryComplete {
        id: root_node_id,
        total_size,
        file_count,
        dir_count,
    });

    let summary = ScanSummary {
        total_files: file_count,
        total_dirs: dir_count + 1,
        total_size,
        inaccessible_count: 0,
    };

    Ok((events, summary, high_usn))
}

fn emit_mft_subtree(
    record_num: u64,
    node_id: NodeId,
    records: &HashMap<u64, ParsedMftRecord>,
    children_map: &HashMap<u64, Vec<u64>>,
    events: &mut Vec<ScanEvent>,
) -> (u64, u64, u64) {
    let mut total_size = 0u64;
    let mut file_count = 0u64;
    let mut dir_count = 0u64;

    let children = match children_map.get(&record_num) {
        Some(c) => c,
        None => return (0, 0, 0),
    };

    for &child_rec in children {
        if child_rec == record_num || child_rec == ROOT_MFT_RECORD {
            continue; // Prevent cycle
        }

        if let Some(record) = records.get(&child_rec) {
            let child_node_id = NodeId(child_rec);

            if record.is_reparse_point {
                // ADR-008 #1: Junctions/symlinks are leaves, never traversed
                events.push(ScanEvent::EnteredDirectory {
                    parent_id: Some(node_id),
                    id: child_node_id,
                    meta: DirMetadata {
                        name: record.file_name.clone(),
                        is_reparse_point: true,
                    },
                });
                events.push(ScanEvent::DirectoryComplete {
                    id: child_node_id,
                    total_size: 0,
                    file_count: 0,
                    dir_count: 0,
                });
                dir_count += 1;
            } else if record.is_directory {
                events.push(ScanEvent::EnteredDirectory {
                    parent_id: Some(node_id),
                    id: child_node_id,
                    meta: DirMetadata {
                        name: record.file_name.clone(),
                        is_reparse_point: false,
                    },
                });

                let (sub_size, sub_files, sub_dirs) =
                    emit_mft_subtree(child_rec, child_node_id, records, children_map, events);

                events.push(ScanEvent::DirectoryComplete {
                    id: child_node_id,
                    total_size: sub_size,
                    file_count: sub_files,
                    dir_count: sub_dirs,
                });

                total_size += sub_size;
                file_count += sub_files;
                dir_count += sub_dirs + 1;
            } else {
                let extension = Path::new(&record.file_name)
                    .extension()
                    .map(|ext| ext.to_string_lossy().into_owned());

                events.push(ScanEvent::FileFound {
                    parent_id: node_id,
                    meta: FileMetadata {
                        name: record.file_name.clone(),
                        size: record.allocated_size,
                        extension,
                        modified: record.modified,
                        created: record.created,
                        attributes: flags_from_raw(record.attributes),
                    },
                });

                total_size += record.allocated_size;
                file_count += 1;
            }
        }
    }

    (total_size, file_count, dir_count)
}
