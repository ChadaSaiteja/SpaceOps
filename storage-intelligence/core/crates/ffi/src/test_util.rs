//! Shared helpers for the per-module FFI tests.

use crate::*;

/// Helper: scan a temp directory and return the raw ScanResultHandle.
pub(crate) unsafe fn scan_temp_dir(dir: &std::path::Path) -> *mut ScanResultHandle {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let cancel = cancel_token_create();
    let mut result: *mut ScanResultHandle = std::ptr::null_mut();
    let mut err_msg: *mut u16 = std::ptr::null_mut();

    let code = scan_drive(
        wide.as_ptr(),
        cancel,
        None,
        std::ptr::null_mut(),
        &mut result,
        &mut err_msg,
    );
    assert_eq!(code, ScanErrorCode::Ok as i32, "scan_drive failed");
    assert!(!result.is_null());
    cancel_token_destroy(cancel);
    result
}
