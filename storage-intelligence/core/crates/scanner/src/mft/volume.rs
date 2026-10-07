//! NTFS volume handle management, volume detection, and permission checks.

use std::io;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDiskFreeSpaceW, GetVolumeInformationW, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

pub struct VolumeHandle(HANDLE);

impl VolumeHandle {
    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for VolumeHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Extracts the drive letter from a root path (e.g. "C:\\" -> 'C').
pub fn extract_drive_letter(root: &Path) -> Option<char> {
    let s = root.to_str()?;
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        let ch = (bytes[0] as char).to_ascii_uppercase();
        if ch.is_ascii_alphabetic() {
            return Some(ch);
        }
    }
    None
}

/// Checks if the volume for the given path is formatted as NTFS.
pub fn is_ntfs_volume(root: &Path) -> bool {
    let drive_letter = match extract_drive_letter(root) {
        Some(d) => d,
        None => return false,
    };
    let root_path_str = format!("{}:\\", drive_letter);
    let root_wide = to_wide_null(&root_path_str);

    let mut fs_name = [0u16; 32];
    let res = unsafe {
        GetVolumeInformationW(
            PCWSTR(root_wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs_name),
        )
    };

    if res.is_ok() {
        let len = fs_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(fs_name.len());
        let name = String::from_utf16_lossy(&fs_name[..len]);
        name.eq_ignore_ascii_case("NTFS")
    } else {
        false
    }
}

/// Opens a raw volume handle (e.g. `\\.\C:`) with read permissions.
/// Requires Administrator privileges (elevated token). Returns io::ErrorKind::PermissionDenied
/// if the process is running as a standard non-elevated user.
pub fn open_volume(drive_letter: char) -> io::Result<VolumeHandle> {
    let volume_path = format!(r"\\.\{}:", drive_letter);
    let wide = to_wide_null(&volume_path);

    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    };

    match handle {
        Ok(h) if !h.is_invalid() && h != INVALID_HANDLE_VALUE => Ok(VolumeHandle(h)),
        _ => Err(io::Error::last_os_error()),
    }
}

/// Obtains the cluster size (allocation unit) for the given drive letter.
pub fn get_volume_cluster_size(drive_letter: char) -> io::Result<u64> {
    let root_path = format!("{}:\\", drive_letter);
    let wide = to_wide_null(&root_path);

    let mut sectors_per_cluster: u32 = 0;
    let mut bytes_per_sector: u32 = 0;
    let mut free_clusters: u32 = 0;
    let mut total_clusters: u32 = 0;

    unsafe {
        GetDiskFreeSpaceW(
            PCWSTR(wide.as_ptr()),
            Some(&mut sectors_per_cluster),
            Some(&mut bytes_per_sector),
            Some(&mut free_clusters),
            Some(&mut total_clusters),
        )
    }
    .map_err(io::Error::from)?;

    Ok(sectors_per_cluster as u64 * bytes_per_sector as u64)
}
