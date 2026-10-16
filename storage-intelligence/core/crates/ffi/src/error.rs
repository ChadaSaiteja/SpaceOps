//! Error codes, panic guard and logging init shared by every exported function (ADR-004/005/007).

use std::path::PathBuf;
use std::sync::Once;

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanErrorCode {
    Ok = 0,
    InvalidPath = 1,
    Cancelled = 2,
    DeviceLost = 3,
    Internal = 4,
}

pub(crate) static LOGGING_INIT: Once = Once::new();

pub(crate) fn init_logging_impl() {
    let log_dir = std::env::var("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("StorageIntelligence").join("logs"))
        .unwrap_or_else(|_| {
            std::env::temp_dir()
                .join("StorageIntelligence")
                .join("logs")
        });
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "core.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // Guard must outlive the process; this cdylib is loaded for the app's whole lifetime.
    Box::leak(Box::new(guard));

    let _ = tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_ansi(false)
        .try_init();
}

/// Idempotent; called automatically on first use, and may also be called explicitly
/// by the host at startup. Owns the single `tracing` subscriber init point (ADR-005).
#[no_mangle]
pub extern "C" fn init_logging() {
    LOGGING_INIT.call_once(init_logging_impl);
}

pub(crate) fn catch_ffi_panic<R>(default: R, f: impl FnOnce() -> R + std::panic::UnwindSafe) -> R {
    match std::panic::catch_unwind(f) {
        Ok(v) => v,
        Err(_) => {
            init_logging_impl_once();
            tracing::error!("panic caught at FFI boundary; converting to internal error code");
            default
        }
    }
}

pub(crate) fn init_logging_impl_once() {
    LOGGING_INIT.call_once(init_logging_impl);
}

pub(crate) fn set_error_message(out: *mut *mut u16, msg: &str) {
    if out.is_null() {
        return;
    }
    let wide: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
    let boxed: Box<[u16]> = wide.into_boxed_slice();
    let ptr = Box::into_raw(boxed) as *mut u16;
    unsafe {
        *out = ptr;
    }
}

/// Frees a message allocated by `scan_drive` via `set_error_message`. Safe to call with
/// a null pointer (no-op).
///
/// # Safety
/// `ptr` must be null, or a pointer previously returned by this crate via an
/// `out_error_message` parameter, and must not have been freed already.
#[no_mangle]
pub unsafe extern "C" fn free_error_message(ptr: *mut u16) {
    catch_ffi_panic(
        (),
        std::panic::AssertUnwindSafe(|| {
            if ptr.is_null() {
                return;
            }
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts_mut(ptr, len + 1);
            drop(Box::from_raw(slice as *mut [u16]));
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_at_ffi_boundary_is_caught_not_propagated() {
        let result = catch_ffi_panic(false, || {
            panic!("simulated internal bug");
            #[allow(unreachable_code)]
            true
        });
        assert!(!result); // default value returned, no unwind escaped this test
    }
}
