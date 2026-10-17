//! UTF-16 string marshalling helpers shared across the FFI modules.

use crate::error::ScanErrorCode;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

pub(crate) unsafe fn wide_ptr_to_pathbuf(ptr: *const u16) -> Option<PathBuf> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    Some(PathBuf::from(std::ffi::OsString::from_wide(slice)))
}

pub(crate) unsafe fn wide_ptr_to_string(ptr: *const u16) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    String::from_utf16(slice).ok()
}

pub(crate) unsafe fn copy_str_to_wide_buf(
    s: &str,
    out_buffer: *mut u16,
    buffer_len: u32,
    out_actual_len: *mut u32,
) -> i32 {
    let utf16: Vec<u16> = s.encode_utf16().collect();
    *out_actual_len = utf16.len() as u32;
    if out_buffer.is_null() || buffer_len == 0 {
        return ScanErrorCode::Ok as i32;
    }
    if (buffer_len as usize) <= utf16.len() {
        return ScanErrorCode::Internal as i32;
    }
    std::ptr::copy_nonoverlapping(utf16.as_ptr(), out_buffer, utf16.len());
    *out_buffer.add(utf16.len()) = 0;
    ScanErrorCode::Ok as i32
}
