//! Cancellation tokens and the blocking scan entry point (ADR-007/008).

use crate::error::{catch_ffi_panic, init_logging_impl_once, set_error_message, ScanErrorCode};
use crate::handles::{CancelHandle, ScanResultHandle};
use crate::marshal::wide_ptr_to_pathbuf;
use common::CancellationToken;
use std::ffi::c_void;
use std::time::Duration;

pub(crate) const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[repr(C)]
pub struct ScanSummaryFfi {
    pub total_files: u64,
    pub total_dirs: u64,
    pub total_size: u64,
    pub inaccessible_count: u64,
}

#[repr(C)]
pub struct ScanProgressFfi {
    pub files_scanned: u64,
    pub bytes_scanned: u64,
}

pub type ProgressCallback = extern "C" fn(ScanProgressFfi, *mut c_void);

/// Wraps a raw `user_data` pointer so it can be captured in a `Send + Sync` closure.
/// The pointer itself is opaque to Rust; thread-safety of what it points to is the
/// caller's (C#'s) responsibility, per the standard FFI callback-with-context pattern.
pub(crate) struct SendPtr(*mut c_void);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

impl SendPtr {
    // A method call (rather than `.0` field access) forces the closure below to capture
    // the whole `SendPtr` wrapper instead of disjointly capturing the raw `*mut c_void`
    // field (RFC 2229 closure capture), which would silently drop our Send/Sync impls.
    pub(crate) fn get(&self) -> *mut c_void {
        self.0
    }
}

#[no_mangle]
pub extern "C" fn cancel_token_create() -> *mut CancelHandle {
    catch_ffi_panic(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(CancelHandle(CancellationToken::new())))
    })
}

#[no_mangle]
/// # Safety
/// `handle` must be null or a valid pointer previously returned by `cancel_token_create`
/// that has not yet been destroyed.
pub unsafe extern "C" fn cancel_token_cancel(handle: *const CancelHandle) {
    catch_ffi_panic(
        (),
        std::panic::AssertUnwindSafe(|| {
            if let Some(h) = handle.as_ref() {
                h.0.cancel();
            }
        }),
    )
}

#[no_mangle]
/// # Safety
/// `handle` must be null or a valid pointer previously returned by `cancel_token_create`,
/// must not be used again after this call, and must not be destroyed twice.
pub unsafe extern "C" fn cancel_token_destroy(handle: *mut CancelHandle) {
    catch_ffi_panic(
        (),
        std::panic::AssertUnwindSafe(|| {
            if !handle.is_null() {
                drop(Box::from_raw(handle));
            }
        }),
    )
}

/// Blocking C ABI entry point (ADR-007): call from a background thread (e.g. a .NET
/// `Task.Run`), never the UI thread. `cancel_handle` may be cancelled concurrently from
/// another thread. On success, `*out_result` receives an owned `ScanResultHandle` the
/// caller must eventually pass to `scan_result_destroy`. On failure, `*out_error_message`
/// (if non-null) receives an owned message the caller must pass to `free_error_message`.
///
/// # Safety
/// `path` must be null or a valid null-terminated UTF-16 string pointer. `cancel_handle`
/// must be a valid pointer from `cancel_token_create`. `out_result` and
/// `out_error_message` must each be valid, writable pointers (or null for the latter).
#[no_mangle]
pub unsafe extern "C" fn scan_drive(
    path: *const u16,
    cancel_handle: *const CancelHandle,
    progress_callback: Option<ProgressCallback>,
    user_data: *mut c_void,
    out_result: *mut *mut ScanResultHandle,
    out_error_message: *mut *mut u16,
) -> i32 {
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
            init_logging_impl_once();

            if out_result.is_null() {
                return ScanErrorCode::Internal as i32;
            }
            *out_result = std::ptr::null_mut();
            if !out_error_message.is_null() {
                *out_error_message = std::ptr::null_mut();
            }

            let path_buf = match wide_ptr_to_pathbuf(path) {
                Some(p) => p,
                None => {
                    set_error_message(out_error_message, "null or invalid path");
                    return ScanErrorCode::InvalidPath as i32;
                }
            };

            let cancel_token = match cancel_handle.as_ref() {
                Some(h) => &h.0,
                None => {
                    set_error_message(out_error_message, "null cancel handle");
                    return ScanErrorCode::Internal as i32;
                }
            };

            let send_user_data = SendPtr(user_data);
            let on_progress = move |p: scanner::ScanProgress| {
                if let Some(cb) = progress_callback {
                    let ffi_progress = ScanProgressFfi {
                        files_scanned: p.files_scanned,
                        bytes_scanned: p.bytes_scanned,
                    };
                    cb(ffi_progress, send_user_data.get());
                }
            };

            match scanner::scan_volume_resilient(
                &path_buf,
                scanner::ScanEngineStrategy::Auto,
                cancel_token,
                PROGRESS_INTERVAL,
                &on_progress,
            ) {
                Ok((events, summary)) => {
                    let handle = Box::new(ScanResultHandle {
                        summary,
                        events: Some(events),
                    });
                    *out_result = Box::into_raw(handle);
                    ScanErrorCode::Ok as i32
                }
                Err(err) => {
                    tracing::error!(error = %err, "scan failed");
                    set_error_message(out_error_message, &err.to_string());
                    match err {
                        common::ScanError::InvalidPath(_) => ScanErrorCode::InvalidPath as i32,
                        common::ScanError::Cancelled => ScanErrorCode::Cancelled as i32,
                        common::ScanError::DeviceLost(_) => ScanErrorCode::DeviceLost as i32,
                        common::ScanError::Internal(_) => ScanErrorCode::Internal as i32,
                    }
                }
            }
        }),
    )
}

#[no_mangle]
/// # Safety
/// `handle` must be null or a valid pointer previously returned by `scan_drive` via
/// `out_result`, and must not have been destroyed.
pub unsafe extern "C" fn scan_result_summary(handle: *const ScanResultHandle) -> ScanSummaryFfi {
    catch_ffi_panic(
        ScanSummaryFfi {
            total_files: 0,
            total_dirs: 0,
            total_size: 0,
            inaccessible_count: 0,
        },
        std::panic::AssertUnwindSafe(|| match handle.as_ref() {
            Some(h) => ScanSummaryFfi {
                total_files: h.summary.total_files,
                total_dirs: h.summary.total_dirs,
                total_size: h.summary.total_size,
                inaccessible_count: h.summary.inaccessible_count,
            },
            None => ScanSummaryFfi {
                total_files: 0,
                total_dirs: 0,
                total_size: 0,
                inaccessible_count: 0,
            },
        }),
    )
}

#[no_mangle]
/// # Safety
/// `handle` must be null or a valid pointer previously returned by `scan_drive` via
/// `out_result`, must not be used again after this call, and must not be destroyed twice.
pub unsafe extern "C" fn scan_result_destroy(handle: *mut ScanResultHandle) {
    catch_ffi_panic(
        (),
        std::panic::AssertUnwindSafe(|| {
            if !handle.is_null() {
                drop(Box::from_raw(handle));
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::free_error_message;

    #[test]
    fn cancel_handle_lifecycle_create_use_destroy() {
        unsafe {
            let handle = cancel_token_create();
            assert!(!handle.is_null());
            assert!(!(*handle).0.is_cancelled());
            cancel_token_cancel(handle);
            assert!((*handle).0.is_cancelled());
            cancel_token_destroy(handle);
        }
    }

    #[test]
    fn scan_drive_end_to_end_on_a_real_temp_directory() {
        use std::os::windows::ffi::OsStrExt;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();

        let wide: Vec<u16> = dir
            .path()
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
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

            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert!(!result.is_null());
            assert!(err_msg.is_null());

            let summary = scan_result_summary(result);
            assert_eq!(summary.total_files, 1);

            scan_result_destroy(result);
            cancel_token_destroy(cancel);
        }
    }

    #[test]
    fn scan_drive_reports_invalid_path_without_panicking() {
        let missing: Vec<u16> = "Z:\\this_should_not_exist_ffi_test\0"
            .encode_utf16()
            .collect();
        unsafe {
            let cancel = cancel_token_create();
            let mut result: *mut ScanResultHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();

            let code = scan_drive(
                missing.as_ptr(),
                cancel,
                None,
                std::ptr::null_mut(),
                &mut result,
                &mut err_msg,
            );

            assert_eq!(code, ScanErrorCode::InvalidPath as i32);
            assert!(result.is_null());
            assert!(!err_msg.is_null());

            free_error_message(err_msg);
            cancel_token_destroy(cancel);
        }
    }
}
