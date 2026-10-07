//! Direct Win32 calls with no safe std equivalent (on-disk allocated size).
//! Confined to this module; unsafe is justified here per project guidelines
//! since std::fs has no wrapper for GetCompressedFileSizeW / GetDiskFreeSpaceW.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
    GetCompressedFileSizeW, GetDiskFreeSpaceW, FIND_FIRST_EX_LARGE_FETCH, WIN32_FIND_DATAW,
};

fn to_wide_null(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Bytes per allocation unit (cluster) for the volume containing `path_on_volume`.
/// `GetDiskFreeSpaceW` accepts any directory on the volume, not just its root.
pub fn cluster_size(path_on_volume: &Path) -> io::Result<u64> {
    let wide = to_wide_null(path_on_volume.as_os_str());
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

/// On-disk allocated size for a compressed or sparse file, via GetCompressedFileSizeW.
pub fn compressed_file_size(path: &Path) -> io::Result<u64> {
    let wide = to_wide_null(path.as_os_str());
    let mut high: u32 = 0;
    let low = unsafe { GetCompressedFileSizeW(PCWSTR(wide.as_ptr()), Some(&mut high)) };

    const INVALID_FILE_SIZE: u32 = 0xFFFF_FFFF;
    if low == INVALID_FILE_SIZE {
        let err = io::Error::last_os_error();
        if let Some(code) = err.raw_os_error() {
            if code != 0 {
                return Err(err);
            }
        }
    }

    Ok(((high as u64) << 32) | low as u64)
}

/// Round a logical size up to the nearest multiple of `cluster_size` (on-disk allocation
/// granularity for ordinary, non-compressed/sparse files). Zero-byte files stay zero.
pub fn round_up_to_cluster(logical_size: u64, cluster_size: u64) -> u64 {
    if logical_size == 0 || cluster_size == 0 {
        return logical_size;
    }
    logical_size.div_ceil(cluster_size) * cluster_size
}

/// Directory entry extracted directly from kernel `WIN32_FIND_DATAW` in a single pass.
/// Eliminates per-entry `symlink_metadata` syscalls entirely.
#[derive(Debug, Clone)]
pub struct FastDirEntry {
    pub name: std::ffi::OsString,
    pub path: std::path::PathBuf,
    pub attributes: u32,
    pub logical_size: u64,
    pub created: Option<std::time::SystemTime>,
    pub modified: Option<std::time::SystemTime>,
    pub is_directory: bool,
    pub is_reparse_point: bool,
}

struct FindHandle(HANDLE);

impl Drop for FindHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = FindClose(self.0);
            }
        }
    }
}

fn filetime_to_system_time(ft_high: u32, ft_low: u32) -> Option<std::time::SystemTime> {
    let intervals = ((ft_high as u64) << 32) | (ft_low as u64);
    if intervals == 0 {
        return None;
    }
    const WINDOWS_TICK: u64 = 10_000_000;
    const SEC_TO_UNIX_EPOCH: u64 = 11_644_473_600;
    let sec_since_windows_epoch = intervals / WINDOWS_TICK;
    if sec_since_windows_epoch < SEC_TO_UNIX_EPOCH {
        return None;
    }
    let sec_since_unix_epoch = sec_since_windows_epoch - SEC_TO_UNIX_EPOCH;
    let nanos = ((intervals % WINDOWS_TICK) * 100) as u32;
    Some(std::time::UNIX_EPOCH + std::time::Duration::new(sec_since_unix_epoch, nanos))
}

fn parse_file_name(c_file_name: &[u16]) -> Option<std::ffi::OsString> {
    let len = c_file_name
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(c_file_name.len());
    if len == 0 {
        return None;
    }
    if len == 1 && c_file_name[0] == b'.' as u16 {
        return None;
    }
    if len == 2 && c_file_name[0] == b'.' as u16 && c_file_name[1] == b'.' as u16 {
        return None;
    }
    use std::os::windows::ffi::OsStringExt;
    Some(std::ffi::OsString::from_wide(&c_file_name[..len]))
}

fn make_search_pattern(dir: &Path) -> Vec<u16> {
    let s_str = dir.to_string_lossy();
    let has_sep = s_str.ends_with('\\') || s_str.ends_with('/');
    let is_prefixed = s_str.starts_with(r"\\?\") || s_str.starts_with(r"\\");

    let mut pattern = if !is_prefixed {
        let mut p = std::ffi::OsString::from(r"\\?\");
        p.push(dir.as_os_str());
        p
    } else {
        dir.as_os_str().to_os_string()
    };

    if !has_sep {
        pattern.push(r"\*");
    } else {
        pattern.push("*");
    }

    to_wide_null(&pattern)
}

/// Enumerate all immediate entries of `dir` using `FindFirstFileExW` with `FindExInfoBasic`
/// and `FIND_FIRST_EX_LARGE_FETCH`. Avoids querying short names (8.3) and fetches large
/// kernel directory batches in single transitions.
pub fn read_directory_fast(dir: &Path) -> io::Result<Vec<FastDirEntry>> {
    let search_pattern = make_search_pattern(dir);
    let mut find_data = WIN32_FIND_DATAW::default();

    let handle = match unsafe {
        FindFirstFileExW(
            PCWSTR(search_pattern.as_ptr()),
            FindExInfoBasic,
            &mut find_data as *mut _ as *mut _,
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        )
    } {
        Ok(h) => h,
        Err(_) => return Err(io::Error::last_os_error()),
    };

    let _guard = FindHandle(handle);
    let mut entries = Vec::new();

    loop {
        if let Some(name) = parse_file_name(&find_data.cFileName) {
            let attrs = find_data.dwFileAttributes;
            let is_directory = (attrs & 0x10) != 0; // FILE_ATTRIBUTE_DIRECTORY
            let is_reparse_point = (attrs & 0x400) != 0; // FILE_ATTRIBUTE_REPARSE_POINT
            let logical_size =
                ((find_data.nFileSizeHigh as u64) << 32) | (find_data.nFileSizeLow as u64);
            let created = filetime_to_system_time(
                find_data.ftCreationTime.dwHighDateTime,
                find_data.ftCreationTime.dwLowDateTime,
            );
            let modified = filetime_to_system_time(
                find_data.ftLastWriteTime.dwHighDateTime,
                find_data.ftLastWriteTime.dwLowDateTime,
            );
            let entry_path = dir.join(&name);

            entries.push(FastDirEntry {
                name,
                path: entry_path,
                attributes: attrs,
                logical_size,
                created,
                modified,
                is_directory,
                is_reparse_point,
            });
        }

        let has_next = unsafe { FindNextFileW(handle, &mut find_data) };
        if has_next.is_err() {
            let err = io::Error::last_os_error();
            // ERROR_NO_MORE_FILES = 18
            if err.raw_os_error() == Some(18) {
                break;
            } else {
                return Err(err);
            }
        }
    }

    Ok(entries)
}
