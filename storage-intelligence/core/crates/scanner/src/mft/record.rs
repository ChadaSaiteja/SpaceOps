//! Binary parser for 1024-byte NTFS MFT file records.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MFT_RECORD_SIZE: usize = 1024;
pub const MFT_MAGIC: u32 = 0x454C_4946; // "FILE" in little-endian

pub const RECORD_FLAG_IN_USE: u16 = 0x0001;
pub const RECORD_FLAG_DIRECTORY: u16 = 0x0002;

pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
pub const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
pub const ATTR_FILE_NAME: u32 = 0x30;
pub const ATTR_DATA: u32 = 0x80;
pub const ATTR_REPARSE_POINT: u32 = 0xC0;
pub const ATTR_END: u32 = 0xFFFF_FFFF;

// File attribute constants from $STANDARD_INFORMATION or $FILE_NAME
pub const FILE_ATTR_READONLY: u32 = 0x0001;
pub const FILE_ATTR_HIDDEN: u32 = 0x0002;
pub const FILE_ATTR_SYSTEM: u32 = 0x0004;
pub const FILE_ATTR_DIRECTORY: u32 = 0x0010;
pub const FILE_ATTR_REPARSE_POINT: u32 = 0x0400;
pub const FILE_ATTR_COMPRESSED: u32 = 0x0800;
pub const FILE_ATTR_SPARSE: u32 = 0x0200;

#[derive(Debug, Clone)]
pub struct ParsedMftRecord {
    pub record_number: u64,
    pub is_in_use: bool,
    pub is_directory: bool,
    pub is_reparse_point: bool,
    pub base_record_number: u64,
    pub parent_record_number: u64,
    pub file_name: OsString,
    pub allocated_size: u64,
    pub logical_size: u64,
    pub attributes: u32,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
}

fn read_u16(buf: &[u8], offset: usize) -> u16 {
    if offset + 2 <= buf.len() {
        u16::from_le_bytes([buf[offset], buf[offset + 1]])
    } else {
        0
    }
}

fn read_u32(buf: &[u8], offset: usize) -> u32 {
    if offset + 4 <= buf.len() {
        u32::from_le_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ])
    } else {
        0
    }
}

fn read_u64(buf: &[u8], offset: usize) -> u64 {
    if offset + 8 <= buf.len() {
        u64::from_le_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
            buf[offset + 4],
            buf[offset + 5],
            buf[offset + 6],
            buf[offset + 7],
        ])
    } else {
        0
    }
}

fn filetime_to_system_time(intervals: u64) -> Option<SystemTime> {
    if intervals == 0 {
        return None;
    }
    const WINDOWS_TICK: u64 = 10_000_000;
    const SEC_TO_UNIX_EPOCH: u64 = 11_644_473_600;
    let sec = intervals / WINDOWS_TICK;
    if sec < SEC_TO_UNIX_EPOCH {
        return None;
    }
    let sec_unix = sec - SEC_TO_UNIX_EPOCH;
    let nanos = ((intervals % WINDOWS_TICK) * 100) as u32;
    Some(UNIX_EPOCH + Duration::new(sec_unix, nanos))
}

/// Applies NTFS fixup array in-place to restore original bytes at the end of each 512-byte sector.
pub fn apply_fixups(record: &mut [u8]) -> bool {
    if record.len() < MFT_RECORD_SIZE {
        return false;
    }
    let magic = read_u32(record, 0);
    if magic != MFT_MAGIC {
        return false;
    }

    let fixup_offset = read_u16(record, 4) as usize;
    let fixup_count = read_u16(record, 6) as usize;

    if fixup_count == 0 || fixup_offset + fixup_count * 2 > record.len() {
        return false;
    }

    let check_word = read_u16(record, fixup_offset);
    let sectors = fixup_count.saturating_sub(1);

    for i in 0..sectors {
        let sector_end_offset = (i + 1) * 512 - 2;
        if sector_end_offset + 2 > record.len() {
            return false;
        }
        let sector_word = read_u16(record, sector_end_offset);
        if sector_word != check_word {
            // Fixup check word mismatch: record is corrupt or modified
            return false;
        }
        let replacement_word = read_u16(record, fixup_offset + (i + 1) * 2);
        record[sector_end_offset] = (replacement_word & 0xFF) as u8;
        record[sector_end_offset + 1] = ((replacement_word >> 8) & 0xFF) as u8;
    }

    true
}

/// Parses an MFT record into a high-level representation.
pub fn parse_mft_record(
    record: &[u8],
    record_number: u64,
    cluster_size: u64,
) -> Option<ParsedMftRecord> {
    if record.len() < 48 {
        return None;
    }
    let magic = read_u32(record, 0);
    if magic != MFT_MAGIC {
        return None;
    }

    let flags = read_u16(record, 22);
    let is_in_use = (flags & RECORD_FLAG_IN_USE) != 0;
    if !is_in_use {
        return None; // Skip unallocated/deleted records
    }
    let is_directory = (flags & RECORD_FLAG_DIRECTORY) != 0;
    let base_record_ref = read_u64(record, 32);
    let base_record_number = base_record_ref & 0x0000_FFFF_FFFF_FFFF;

    let mut attr_offset = read_u16(record, 20) as usize;
    let mut file_name = OsString::new();
    let mut parent_record_number = 0u64;
    let mut allocated_size = 0u64;
    let mut logical_size = 0u64;
    let mut attributes = 0u32;
    let mut created = None;
    let mut modified = None;
    let mut is_reparse_point = false;

    // Track best name namespace: Win32 (1) and Win32/DOS (3) take precedence over DOS (2) or POSIX (0)
    let mut best_namespace = 255u8;

    while attr_offset + 8 <= record.len() {
        let attr_type = read_u32(record, attr_offset);
        if attr_type == ATTR_END || attr_type == 0 {
            break;
        }
        let attr_len = read_u32(record, attr_offset + 4) as usize;
        if attr_len == 0 || attr_offset + attr_len > record.len() {
            break;
        }

        let non_resident = record[attr_offset + 8] != 0;

        match attr_type {
            ATTR_STANDARD_INFORMATION => {
                if !non_resident && attr_offset + 16 <= record.len() {
                    let val_offset = attr_offset + read_u16(record, attr_offset + 20) as usize;
                    if val_offset + 48 <= attr_offset + attr_len {
                        created = filetime_to_system_time(read_u64(record, val_offset));
                        modified = filetime_to_system_time(read_u64(record, val_offset + 8));
                        attributes |= read_u32(record, val_offset + 32);
                    }
                }
            }
            ATTR_FILE_NAME => {
                if !non_resident && attr_offset + 16 <= record.len() {
                    let val_offset = attr_offset + read_u16(record, attr_offset + 20) as usize;
                    if val_offset + 66 <= attr_offset + attr_len {
                        let parent_ref = read_u64(record, val_offset);
                        let name_len = record[val_offset + 64] as usize;
                        let namespace = record[val_offset + 65];

                        if val_offset + 66 + name_len * 2 <= attr_offset + attr_len {
                            // Pick Win32 (1) or Win32+DOS (3) or fallback
                            if namespace == 1 || namespace == 3 || best_namespace == 255 {
                                best_namespace = namespace;
                                parent_record_number = parent_ref & 0x0000_FFFF_FFFF_FFFF;

                                let mut wide_chars = Vec::with_capacity(name_len);
                                for c in 0..name_len {
                                    wide_chars.push(read_u16(record, val_offset + 66 + c * 2));
                                }
                                file_name = OsString::from_wide(&wide_chars);

                                let fn_flags = read_u32(record, val_offset + 56);
                                attributes |= fn_flags;
                            }
                        }
                    }
                }
            }
            ATTR_DATA => {
                // If named data stream, skip (we only count default unnamed data stream)
                let name_len = record[attr_offset + 9] as usize;
                if name_len == 0 {
                    if non_resident {
                        if attr_offset + 48 <= record.len() {
                            let alloc = read_u64(record, attr_offset + 40);
                            let real = read_u64(record, attr_offset + 48);
                            allocated_size = alloc;
                            logical_size = real;
                        }
                    } else {
                        let val_len = read_u32(record, attr_offset + 16) as u64;
                        logical_size = val_len;
                        // Resident data takes on-disk cluster multiple if non-zero
                        allocated_size = if val_len > 0 && cluster_size > 0 {
                            cluster_size
                        } else {
                            0
                        };
                    }
                }
            }
            ATTR_REPARSE_POINT => {
                is_reparse_point = true;
            }
            _ => {}
        }

        attr_offset += attr_len;
    }

    if (attributes & FILE_ATTR_REPARSE_POINT) != 0 {
        is_reparse_point = true;
    }

    Some(ParsedMftRecord {
        record_number,
        is_in_use,
        is_directory,
        is_reparse_point,
        base_record_number,
        parent_record_number,
        file_name,
        allocated_size,
        logical_size,
        attributes,
        created,
        modified,
    })
}
