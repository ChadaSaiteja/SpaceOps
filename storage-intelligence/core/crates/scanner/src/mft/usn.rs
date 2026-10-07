//! NTFS USN Change Journal reader for sub-50ms incremental updates.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Ioctl::{
    FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_USN_JOURNAL, READ_USN_JOURNAL_DATA_V0, USN_JOURNAL_DATA_V0,
    USN_RECORD_V2,
};
use windows::Win32::System::IO::DeviceIoControl;

pub const USN_REASON_DATA_OVERWRITE: u32 = 0x0000_0001;
pub const USN_REASON_DATA_EXTEND: u32 = 0x0000_0002;
pub const USN_REASON_DATA_TRUNCATION: u32 = 0x0000_0004;
pub const USN_REASON_FILE_CREATE: u32 = 0x0000_0100;
pub const USN_REASON_FILE_DELETE: u32 = 0x0000_0200;
pub const USN_REASON_RENAME_OLD_NAME: u32 = 0x0000_1000;
pub const USN_REASON_RENAME_NEW_NAME: u32 = 0x0000_2000;
pub const USN_REASON_CLOSE: u32 = 0x8000_0000;

#[derive(Debug, Clone)]
pub struct UsnChangeRecord {
    pub usn: i64,
    pub file_ref: u64,
    pub parent_ref: u64,
    pub reason: u32,
    pub file_attributes: u32,
    pub file_name: OsString,
}

#[derive(Debug, Clone)]
pub struct UsnJournalInfo {
    pub journal_id: u64,
    pub next_usn: i64,
    pub lowest_valid_usn: i64,
}

/// Query current USN journal status, returning the active journal ID and latest USN.
pub fn query_usn_journal(volume_handle: HANDLE) -> io::Result<UsnJournalInfo> {
    let mut journal_data = USN_JOURNAL_DATA_V0::default();
    let mut bytes_returned = 0u32;

    let success = unsafe {
        DeviceIoControl(
            volume_handle,
            FSCTL_QUERY_USN_JOURNAL,
            None,
            0,
            Some(&mut journal_data as *mut _ as *mut _),
            std::mem::size_of::<USN_JOURNAL_DATA_V0>() as u32,
            Some(&mut bytes_returned),
            None,
        )
    };

    if success.is_err() {
        return Err(io::Error::last_os_error());
    }

    Ok(UsnJournalInfo {
        journal_id: journal_data.UsnJournalID,
        next_usn: journal_data.NextUsn,
        lowest_valid_usn: journal_data.LowestValidUsn,
    })
}

/// Read all USN records between `start_usn` and the current journal tip.
pub fn read_usn_changes(
    volume_handle: HANDLE,
    journal_id: u64,
    start_usn: i64,
) -> io::Result<(Vec<UsnChangeRecord>, i64)> {
    let mut read_data = READ_USN_JOURNAL_DATA_V0 {
        StartUsn: start_usn,
        ReasonMask: 0xFFFF_FFFF,
        ReturnOnlyOnClose: 0,
        Timeout: 0,
        BytesToWaitFor: 0,
        UsnJournalID: journal_id,
    };

    let mut buffer = vec![0u8; 64 * 1024]; // 64 KB buffer
    let mut records = Vec::new();
    let mut current_usn = start_usn;

    loop {
        let mut bytes_returned = 0u32;
        let success = unsafe {
            DeviceIoControl(
                volume_handle,
                FSCTL_READ_USN_JOURNAL,
                Some(&read_data as *const _ as *const _),
                std::mem::size_of::<READ_USN_JOURNAL_DATA_V0>() as u32,
                Some(buffer.as_mut_ptr() as *mut _),
                buffer.len() as u32,
                Some(&mut bytes_returned),
                None,
            )
        };

        if success.is_err() {
            let err = io::Error::last_os_error();
            return Err(err);
        }

        if bytes_returned < 8 {
            break;
        }

        // The first 8 bytes of the output buffer contain the next USN to read
        let next_start_usn = i64::from_le_bytes(buffer[0..8].try_into().unwrap());
        if next_start_usn <= current_usn {
            break;
        }

        let mut offset = 8usize;
        while offset + std::mem::size_of::<USN_RECORD_V2>() <= bytes_returned as usize {
            let record_len =
                u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap()) as usize;
            if record_len == 0 || offset + record_len > bytes_returned as usize {
                break;
            }

            let major_version =
                u16::from_le_bytes(buffer[offset + 4..offset + 6].try_into().unwrap());
            if major_version == 2 {
                let file_ref =
                    u64::from_le_bytes(buffer[offset + 8..offset + 16].try_into().unwrap())
                        & 0x0000_FFFF_FFFF_FFFF;
                let parent_ref =
                    u64::from_le_bytes(buffer[offset + 16..offset + 24].try_into().unwrap())
                        & 0x0000_FFFF_FFFF_FFFF;
                let usn = i64::from_le_bytes(buffer[offset + 24..offset + 32].try_into().unwrap());
                let reason =
                    u32::from_le_bytes(buffer[offset + 40..offset + 44].try_into().unwrap());
                let file_attrs =
                    u32::from_le_bytes(buffer[offset + 52..offset + 56].try_into().unwrap());
                let name_len =
                    u16::from_le_bytes(buffer[offset + 56..offset + 58].try_into().unwrap())
                        as usize;
                let name_offset =
                    u16::from_le_bytes(buffer[offset + 58..offset + 60].try_into().unwrap())
                        as usize;

                let name_start = offset + name_offset;
                let name_end = name_start + name_len;
                let file_name = if name_end <= offset + record_len {
                    #[allow(clippy::chunks_exact_to_as_chunks)]
                    let chars: Vec<u16> = buffer[name_start..name_end]
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    OsString::from_wide(&chars)
                } else {
                    OsString::new()
                };

                records.push(UsnChangeRecord {
                    usn,
                    file_ref,
                    parent_ref,
                    reason,
                    file_attributes: file_attrs,
                    file_name,
                });
            }

            offset += record_len;
        }

        current_usn = next_start_usn;
        read_data.StartUsn = current_usn;

        // If returned bytes is small, we reached current journal tail
        if (bytes_returned as usize) <= 8 {
            break;
        }
    }

    Ok((records, current_usn))
}
