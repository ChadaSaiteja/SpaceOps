//! C ABI boundary (ADR-001). Only crate allowed to use unsafe for the Rust<->C# bridge
//! (as opposed to `scanner`'s `winfs` module, which uses unsafe for Rust<->Win32 calls;
//! see ADR-003 note). Every exported function is panic-guarded (ADR-004/007): a panic
//! anywhere in scanner logic never unwinds across this boundary.
//!
//! Note on panic safety and rayon: `scanner::scan`'s internal parallelism is built on
//! `rayon`'s structured `par_iter`/`reduce`, which re-raises any worker-thread panic on
//! the thread that calls `.reduce()` (i.e. back inside this same call stack). A single
//! `catch_unwind` at each exported function's entry point is therefore sufficient; no
//! additional per-callback-site guarding inside `scanner` is needed.

use common::CancellationToken;
use std::ffi::c_void;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::Once;
use std::time::Duration;

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanErrorCode {
    Ok = 0,
    InvalidPath = 1,
    Cancelled = 2,
    DeviceLost = 3,
    Internal = 4,
}

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

/// Opaque handle wrapping a `CancellationToken`, owned by the caller until
/// `cancel_token_destroy`. Safe to call `cancel_token_cancel` from a different
/// thread than the one blocked inside `scan_drive` (ADR-007's threading model).
pub struct CancelHandle(CancellationToken);

/// Opaque handle wrapping a completed scan's summary and event stream, owned by
/// the caller until `scan_result_destroy`. Events are consumed (moved) by
/// `tree_create` to build the storage tree (ADR-009 #5: two-handle lifecycle).
pub struct ScanResultHandle {
    summary: common::ScanSummary,
    /// `Some` until `tree_create` moves the events out; `None` afterward.
    events: Option<Vec<common::ScanEvent>>,
}

/// Opaque handle wrapping a built `StorageTree` (ADR-009). Owned by the caller
/// until `tree_destroy`. Thread-safe for concurrent reads.
pub struct TreeHandle(storage_tree::StorageTree);

/// Wraps a raw `user_data` pointer so it can be captured in a `Send + Sync` closure.
/// The pointer itself is opaque to Rust; thread-safety of what it points to is the
/// caller's (C#'s) responsibility, per the standard FFI callback-with-context pattern.
struct SendPtr(*mut c_void);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

impl SendPtr {
    // A method call (rather than `.0` field access) forces the closure below to capture
    // the whole `SendPtr` wrapper instead of disjointly capturing the raw `*mut c_void`
    // field (RFC 2229 closure capture), which would silently drop our Send/Sync impls.
    fn get(&self) -> *mut c_void {
        self.0
    }
}

static LOGGING_INIT: Once = Once::new();

fn init_logging_impl() {
    let log_dir = std::env::var("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("StorageIntelligence").join("logs"))
        .unwrap_or_else(|_| std::env::temp_dir().join("StorageIntelligence").join("logs"));
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

fn catch_ffi_panic<R>(default: R, f: impl FnOnce() -> R + std::panic::UnwindSafe) -> R {
    match std::panic::catch_unwind(f) {
        Ok(v) => v,
        Err(_) => {
            init_logging_impl_once();
            tracing::error!("panic caught at FFI boundary; converting to internal error code");
            default
        }
    }
}

fn init_logging_impl_once() {
    LOGGING_INIT.call_once(init_logging_impl);
}

unsafe fn wide_ptr_to_pathbuf(ptr: *const u16) -> Option<PathBuf> {
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

unsafe fn wide_ptr_to_string(ptr: *const u16) -> Option<String> {
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

fn set_error_message(out: *mut *mut u16, msg: &str) {
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
    catch_ffi_panic((), std::panic::AssertUnwindSafe(|| {
        if ptr.is_null() {
            return;
        }
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts_mut(ptr, len + 1);
        drop(Box::from_raw(slice as *mut [u16]));
    }))
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
    catch_ffi_panic((), std::panic::AssertUnwindSafe(|| {
        if let Some(h) = handle.as_ref() {
            h.0.cancel();
        }
    }))
}

#[no_mangle]
/// # Safety
/// `handle` must be null or a valid pointer previously returned by `cancel_token_create`,
/// must not be used again after this call, and must not be destroyed twice.
pub unsafe extern "C" fn cancel_token_destroy(handle: *mut CancelHandle) {
    catch_ffi_panic((), std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            drop(Box::from_raw(handle));
        }
    }))
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
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
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
                let handle = Box::new(ScanResultHandle { summary, events: Some(events) });
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
    }))
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
    catch_ffi_panic((), std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            drop(Box::from_raw(handle));
        }
    }))
}

// ---------------------------------------------------------------------------
// Phase 3: Storage Tree FFI exports (ADR-009)
// ---------------------------------------------------------------------------

/// C-compatible node info struct. Data is copied from the tree into this struct;
/// string pointers (`name`, `extension`) point into caller-provided buffers or
/// are encoded inline as UTF-16. For simplicity at MVP, names are copied into
/// a heap-allocated UTF-16 buffer that lives until `tree_destroy`.
#[repr(C)]
pub struct NodeInfoFfi {
    pub id: u64,
    pub parent_id: u64,       // u64::MAX if root (no parent)
    pub kind: i32,            // 0=Dir, 1=File, 2=ReparsePoint, 3=Inaccessible
    pub size: u64,
    pub file_count: u64,
    pub dir_count: u64,
}

/// C-compatible representation of a single laid-out rectangle in the treemap (ADR-010).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreemapRectFfi {
    pub node_id: u64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub depth: u32,
    pub kind: i32,           // 0=Dir, 1=File, 2=ReparsePoint, 3=Inaccessible
    pub category: i32,       // 0=Video, 1=Audio, 2=Image, 3=Document, 4=Archive, 5=Executable, 6=Code, 7=System, 8=Other
}

/// C-compatible representation of a search result hit (ADR-012).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchResultFfi {
    pub node_id: u64,
    pub size: u64,
    pub kind: i32,           // 0=Dir, 1=File, 2=ReparsePoint, 3=Inaccessible
    pub category: i32,       // 0=Video, 1=Audio, 2=Image, 3=Document, 4=Archive, 5=Executable, 6=Code, 7=System, 8=Other
    pub score: u32,
}

/// C-compatible representation of a cleanup candidate group (ADR-013).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanupCandidateFfi {
    pub rule_id: u32,
    pub risk_level: i32,
    pub total_bytes: u64,
    pub file_count: u64,
    pub is_protected: u8,
}

/// C-compatible execution report for a cleanup operation (ADR-013).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanupReportFfi {
    pub files_reclaimed: u64,
    pub bytes_reclaimed: u64,
    pub files_failed: u64,
    pub is_dry_run: u8,
}

fn node_kind_to_i32(kind: storage_tree::NodeKind) -> i32 {
    match kind {
        storage_tree::NodeKind::Directory => 0,
        storage_tree::NodeKind::File => 1,
        storage_tree::NodeKind::ReparsePoint => 2,
        storage_tree::NodeKind::Inaccessible => 3,
    }
}

/// Build a `StorageTree` from the events in `scan_result`. The events are **moved**
/// (not copied) out of the `ScanResultHandle`; calling `tree_create` a second time
/// on the same result handle returns `ScanErrorCode::Internal`.
///
/// On success, `*out_tree` receives an owned `TreeHandle`. The caller must eventually
/// pass it to `tree_destroy`.
///
/// # Safety
/// `scan_result` must be a valid, non-null pointer from `scan_drive`. `out_tree` must
/// be a valid, writable pointer.
#[no_mangle]
pub unsafe extern "C" fn tree_create(
    scan_result: *mut ScanResultHandle,
    out_tree: *mut *mut TreeHandle,
    out_error_message: *mut *mut u16,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        init_logging_impl_once();

        if out_tree.is_null() || scan_result.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        *out_tree = std::ptr::null_mut();
        if !out_error_message.is_null() {
            *out_error_message = std::ptr::null_mut();
        }

        let result_ref = &mut *scan_result;
        let events = match result_ref.events.take() {
            Some(e) => e,
            None => {
                set_error_message(out_error_message, "events already consumed by a previous tree_create call");
                return ScanErrorCode::Internal as i32;
            }
        };

        match storage_tree::StorageTree::build(events) {
            Ok(tree) => {
                *out_tree = Box::into_raw(Box::new(TreeHandle(tree)));
                ScanErrorCode::Ok as i32
            }
            Err(err) => {
                tracing::error!(error = %err, "tree build failed");
                set_error_message(out_error_message, &err.to_string());
                ScanErrorCode::Internal as i32
            }
        }
    }))
}

#[no_mangle]
/// # Safety
/// `handle` must be null or a valid pointer from `tree_create`, not yet destroyed.
pub unsafe extern "C" fn tree_destroy(handle: *mut TreeHandle) {
    catch_ffi_panic((), std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            drop(Box::from_raw(handle));
        }
    }))
}

#[no_mangle]
pub unsafe extern "C" fn tree_root_id(handle: *const TreeHandle) -> u64 {
    catch_ffi_panic(u64::MAX, std::panic::AssertUnwindSafe(|| {
        match handle.as_ref() {
            Some(h) => h.0.root().0,
            None => u64::MAX,
        }
    }))
}

#[no_mangle]
pub unsafe extern "C" fn tree_node_count(handle: *const TreeHandle) -> u64 {
    catch_ffi_panic(0, std::panic::AssertUnwindSafe(|| {
        match handle.as_ref() {
            Some(h) => h.0.node_count() as u64,
            None => 0,
        }
    }))
}

/// Get info for a single node. Returns 0 on success, non-zero if invalid.
///
/// # Safety
/// `handle` must be valid from `tree_create`. `out_info` must be a valid, writable pointer.
#[no_mangle]
pub unsafe extern "C" fn tree_node_info(
    handle: *const TreeHandle,
    node_id: u64,
    out_info: *mut NodeInfoFfi,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || out_info.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let tree = &(*handle).0;
        match tree.node(common::NodeId(node_id)) {
            Some(info) => {
                *out_info = NodeInfoFfi {
                    id: info.id.0,
                    parent_id: info.parent.map_or(u64::MAX, |p| p.0),
                    kind: node_kind_to_i32(info.kind),
                    size: info.size,
                    file_count: info.file_count,
                    dir_count: info.dir_count,
                };
                ScanErrorCode::Ok as i32
            }
            None => ScanErrorCode::InvalidPath as i32, // reusing error code for "not found"
        }
    }))
}

/// Get children of a node (pre-sorted by size desc). Paginated via offset/limit.
///
/// # Safety
/// `out_ids` must point to a buffer of at least `limit` u64s. `out_count` and
/// `out_total_children` must be valid, writable pointers.
#[no_mangle]
pub unsafe extern "C" fn tree_children(
    handle: *const TreeHandle,
    node_id: u64,
    offset: u32,
    limit: u32,
    out_ids: *mut u64,
    out_count: *mut u32,
    out_total_children: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || out_ids.is_null() || out_count.is_null() || out_total_children.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let tree = &(*handle).0;
        match tree.children(common::NodeId(node_id)) {
            Some(children) => {
                *out_total_children = children.len() as u32;
                let start = (offset as usize).min(children.len());
                let end = (start + limit as usize).min(children.len());
                let page = &children[start..end];
                for (i, child_id) in page.iter().enumerate() {
                    *out_ids.add(i) = child_id.0;
                }
                *out_count = page.len() as u32;
                ScanErrorCode::Ok as i32
            }
            None => {
                *out_count = 0;
                *out_total_children = 0;
                ScanErrorCode::InvalidPath as i32
            }
        }
    }))
}

/// Get ancestors of a node (bottom-up: node → parent → ... → root).
///
/// # Safety
/// `out_ids` must point to a buffer of at least `limit` u64s.
#[no_mangle]
pub unsafe extern "C" fn tree_ancestors(
    handle: *const TreeHandle,
    node_id: u64,
    out_ids: *mut u64,
    limit: u32,
    out_count: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || out_ids.is_null() || out_count.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let tree = &(*handle).0;
        let ancestors = tree.ancestors(common::NodeId(node_id));
        let take = (limit as usize).min(ancestors.len());
        for (i, id) in ancestors[..take].iter().enumerate() {
            *out_ids.add(i) = id.0;
        }
        *out_count = take as u32;
        ScanErrorCode::Ok as i32
    }))
}

/// Get top N largest files under a subtree.
///
/// # Safety
/// `out_ids` must point to a buffer of at least `limit` u64s.
#[no_mangle]
pub unsafe extern "C" fn tree_top_files_by_size(
    handle: *const TreeHandle,
    root_id: u64,
    limit: u32,
    out_ids: *mut u64,
    out_count: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || out_ids.is_null() || out_count.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let tree = &(*handle).0;
        let top = tree.top_files_by_size(common::NodeId(root_id), limit as usize);
        for (i, id) in top.iter().enumerate() {
            *out_ids.add(i) = id.0;
        }
        *out_count = top.len() as u32;
        ScanErrorCode::Ok as i32
    }))
}

/// Copies the node's name as null-terminated UTF-16 into `out_buffer`.
/// `buffer_len` is in u16 units (including null terminator).
/// `*out_actual_len` receives the UTF-16 length in characters (excluding null terminator).
///
/// # Safety
/// `handle` must be valid from `tree_create`. `out_actual_len` must be a valid, writable pointer.
/// If `out_buffer` is non-null, it must point to writable memory of at least `buffer_len` u16s.
#[no_mangle]
pub unsafe extern "C" fn tree_node_name(
    handle: *const TreeHandle,
    node_id: u64,
    out_buffer: *mut u16,
    buffer_len: u32,
    out_actual_len: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || out_actual_len.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let tree = &(*handle).0;
        match tree.node(common::NodeId(node_id)) {
            Some(info) => {
                let utf16: Vec<u16> = info.name.encode_utf16().collect();
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
            None => ScanErrorCode::InvalidPath as i32,
        }
    }))
}

/// Reconstructs the node's full path as null-terminated UTF-16 into `out_buffer`.
/// `buffer_len` is in u16 units (including null terminator).
/// `*out_actual_len` receives the UTF-16 length in characters (excluding null terminator).
///
/// # Safety
/// `handle` must be valid from `tree_create`. `out_actual_len` must be a valid, writable pointer.
/// If `out_buffer` is non-null, it must point to writable memory of at least `buffer_len` u16s.
#[no_mangle]
pub unsafe extern "C" fn tree_node_path(
    handle: *const TreeHandle,
    node_id: u64,
    out_buffer: *mut u16,
    buffer_len: u32,
    out_actual_len: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || out_actual_len.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let tree = &(*handle).0;
        match tree.full_path(common::NodeId(node_id)) {
            Some(path) => {
                let path_str = path.to_string_lossy();
                let utf16: Vec<u16> = path_str.encode_utf16().collect();
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
            None => ScanErrorCode::InvalidPath as i32,
        }
    }))
}

/// Finds a node by absolute filesystem path within the active StorageTree.
/// Returns Ok (0) and writes the node_id to `*out_node_id`, or InvalidPath if not found.
///
/// # Safety
/// `handle`, `path`, and `out_node_id` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn tree_find_by_path(
    handle: *const TreeHandle,
    path: *const u16,
    out_node_id: *mut u64,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || path.is_null() || out_node_id.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let path_buf = match wide_ptr_to_pathbuf(path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };
        let tree = &(*handle).0;
        match tree.find_by_path(&path_buf) {
            Some(id) => {
                *out_node_id = id.0;
                ScanErrorCode::Ok as i32
            }
            None => ScanErrorCode::InvalidPath as i32,
        }
    }))
}

/// Computes squarified treemap layout starting from `root_id` into `out_rects`.
/// `limit` is the capacity of the `out_rects` buffer in elements.
/// `*out_count` receives the number of rectangles written into `out_rects`.
/// `*out_total_rects` receives the total number of rectangles generated by the layout.
///
/// # Safety
/// `handle` must be valid from `tree_create`. `out_count` and `out_total_rects` must be valid, writable pointers.
/// If `limit > 0` and `out_rects` is non-null, it must point to a buffer of at least `limit` TreemapRectFfi items.
#[no_mangle]
pub unsafe extern "C" fn tree_compute_layout(
    handle: *const TreeHandle,
    root_id: u64,
    width: f32,
    height: f32,
    max_depth: u32,
    min_size_px: f32,
    out_rects: *mut TreemapRectFfi,
    limit: u32,
    out_count: *mut u32,
    out_total_rects: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || out_count.is_null() || out_total_rects.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let tree = &(*handle).0;
        let rects = tree.compute_layout(
            common::NodeId(root_id),
            width,
            height,
            max_depth,
            min_size_px,
        );

        *out_total_rects = rects.len() as u32;

        if out_rects.is_null() || limit == 0 {
            *out_count = 0;
            return ScanErrorCode::Ok as i32;
        }

        let write_count = (limit as usize).min(rects.len());
        for (i, r) in rects[..write_count].iter().enumerate() {
            *out_rects.add(i) = TreemapRectFfi {
                node_id: r.node_id.0,
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
                depth: r.depth,
                kind: node_kind_to_i32(r.kind),
                category: r.category as i32,
            };
        }

        *out_count = write_count as u32;
        ScanErrorCode::Ok as i32
    }))
}

/// Searches the StorageTree arena using the given query (ADR-012).
/// Supports tokenized filters (ext:, size:, type:, kind:) and space-aware ranking.
///
/// # Safety
/// `handle` must be valid from `tree_create`. `query` must be a null-terminated UTF-16 string.
/// `out_count` must be a valid, writable pointer.
/// If `limit > 0` and `out_results` is non-null, it must point to a buffer of at least `limit` SearchResultFfi items.
#[no_mangle]
pub unsafe extern "C" fn tree_search(
    handle: *const TreeHandle,
    query: *const u16,
    limit: u32,
    out_results: *mut SearchResultFfi,
    out_count: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || query.is_null() || out_count.is_null() {
            return ScanErrorCode::Internal as i32;
        }

        let query_str = match wide_ptr_to_string(query) {
            Some(s) => s,
            None => return ScanErrorCode::Internal as i32,
        };

        let parsed_query = storage_tree::SearchQuery::parse(&query_str);
        let tree = &(*handle).0;
        let hits = tree.search(&parsed_query, limit as usize);

        if out_results.is_null() || limit == 0 {
            *out_count = hits.len() as u32;
            return ScanErrorCode::Ok as i32;
        }

        let write_count = (limit as usize).min(hits.len());
        for (i, hit) in hits[..write_count].iter().enumerate() {
            *out_results.add(i) = SearchResultFfi {
                node_id: hit.node_id.0,
                size: hit.size,
                kind: node_kind_to_i32(hit.kind),
                category: hit.category as i32,
                score: hit.score,
            };
        }

        *out_count = write_count as u32;
        ScanErrorCode::Ok as i32
    }))
}

/// Detects all cleanup candidate categories across the active in-memory tree (ADR-013).
///
/// # Safety
/// `handle` must be a valid pointer from `tree_create`. `out_count` and `out_total` must be valid writable pointers.
/// If `limit > 0` and `out_candidates` is non-null, it must point to a buffer of at least `limit` CleanupCandidateFfi items.
#[no_mangle]
pub unsafe extern "C" fn cleanup_detect_candidates(
    handle: *const TreeHandle,
    out_candidates: *mut CleanupCandidateFfi,
    limit: u32,
    out_count: *mut u32,
    out_total: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || out_count.is_null() || out_total.is_null() {
            return ScanErrorCode::Internal as i32;
        }

        let tree = &(*handle).0;
        let candidates = tree.detect_cleanup_candidates();

        *out_total = candidates.len() as u32;

        if out_candidates.is_null() || limit == 0 {
            *out_count = 0;
            return ScanErrorCode::Ok as i32;
        }

        let write_count = (limit as usize).min(candidates.len());
        for (i, c) in candidates[..write_count].iter().enumerate() {
            *out_candidates.add(i) = CleanupCandidateFfi {
                rule_id: c.rule_id as u32,
                risk_level: c.risk_level as i32,
                total_bytes: c.total_bytes,
                file_count: c.file_count,
                is_protected: if c.is_protected { 1 } else { 0 },
            };
        }

        *out_count = write_count as u32;
        ScanErrorCode::Ok as i32
    }))
}

/// Executes or simulates cleanup for a specific candidate category (ADR-013).
/// Enforces safety invariants: protected paths are rejected, deletions default to Recycle Bin.
///
/// # Safety
/// `handle` must be a valid pointer from `tree_create`. `out_report` must be a valid writable pointer.
#[no_mangle]
pub unsafe extern "C" fn cleanup_execute_rule(
    handle: *const TreeHandle,
    rule_id: u32,
    dry_run: u8,
    send_to_recycle_bin: u8,
    out_report: *mut CleanupReportFfi,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || out_report.is_null() {
            return ScanErrorCode::Internal as i32;
        }

        let rule = match storage_tree::CleanupRuleId::from_u32(rule_id) {
            Some(r) => r,
            None => return ScanErrorCode::InvalidPath as i32,
        };

        let tree = &(*handle).0;
        let candidates = tree.detect_cleanup_candidates();
        let target_candidate = candidates.into_iter().find(|c| c.rule_id == rule);

        let report = match target_candidate {
            Some(cand) => {
                storage_tree::cleanup::execute_cleanup(&cand.target_paths, dry_run != 0, send_to_recycle_bin != 0)
            }
            None => storage_tree::CleanupReport::default(),
        };

        *out_report = CleanupReportFfi {
            files_reclaimed: report.files_reclaimed,
            bytes_reclaimed: report.bytes_reclaimed,
            files_failed: report.files_failed,
            is_dry_run: if report.is_dry_run { 1 } else { 0 },
        };

        ScanErrorCode::Ok as i32
    }))
}

/// Checks if a filesystem path is an unbypassable protected system location (ADR-013).
///
/// # Safety
/// `path` must be a null-terminated UTF-16 string pointer.
#[no_mangle]
pub unsafe extern "C" fn cleanup_is_path_protected(path: *const u16) -> u8 {
    catch_ffi_panic(1u8, std::panic::AssertUnwindSafe(|| {
        if path.is_null() {
            return 1u8;
        }
        match wide_ptr_to_pathbuf(path) {
            Some(p) => if storage_tree::is_path_protected(&p) { 1u8 } else { 0u8 },
            None => 1u8,
        }
    }))
}

// ---------------------------------------------------------------------------
// Phase 7: Application Manager & Storage Attribution FFI (ADR-014)
// ---------------------------------------------------------------------------

/// Opaque handle wrapping discovered applications (ADR-014).
pub struct AppCatalogHandle(Vec<storage_tree::AppInfo>);

/// Opaque handle wrapping discovered leftovers (ADR-014).
pub struct AppLeftoversHandle(Vec<storage_tree::AppLeftover>);

#[repr(C)]
pub struct AppInfoFfi {
    pub kind: u32,
    pub estimated_size: u64,
    pub actual_size: u64,
    pub file_count: u64,
    pub is_system_component: u8,
}

#[repr(C)]
pub struct AppLeftoverFfi {
    pub size: u64,
    pub file_count: u64,
    pub location_type: u32,
    pub risk_level: i32,
}

unsafe fn copy_str_to_wide_buf(
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

/// Discovers installed applications and attributes storage using the active tree (ADR-014).
/// `tree_handle` is optional (may be null).
/// `*out_catalog` receives an owned handle the caller must destroy via `app_catalog_destroy`.
///
/// # Safety
/// `out_catalog` must be a valid, writable pointer.
#[no_mangle]
pub unsafe extern "C" fn app_catalog_create(
    tree_handle: *const TreeHandle,
    out_catalog: *mut *mut AppCatalogHandle,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if out_catalog.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        *out_catalog = std::ptr::null_mut();

        let tree = if !tree_handle.is_null() {
            Some(&(*tree_handle).0)
        } else {
            None
        };

        let apps = storage_tree::app_manager::discover_win32_apps(tree);
        let boxed = Box::new(AppCatalogHandle(apps));
        *out_catalog = Box::into_raw(boxed);

        ScanErrorCode::Ok as i32
    }))
}

/// Returns the number of applications in the catalog.
///
/// # Safety
/// `catalog` must be null or a valid pointer from `app_catalog_create`.
#[no_mangle]
pub unsafe extern "C" fn app_catalog_count(catalog: *const AppCatalogHandle) -> u32 {
    catch_ffi_panic(0u32, std::panic::AssertUnwindSafe(|| {
        if catalog.is_null() {
            0u32
        } else {
            (*catalog).0.len() as u32
        }
    }))
}

/// Retrieves metadata numbers for the application at `index`.
///
/// # Safety
/// `catalog` must be valid from `app_catalog_create`. `out_info` must be a valid writable pointer.
#[no_mangle]
pub unsafe extern "C" fn app_catalog_item(
    catalog: *const AppCatalogHandle,
    index: u32,
    out_info: *mut AppInfoFfi,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if catalog.is_null() || out_info.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let list = &(*catalog).0;
        let idx = index as usize;
        if idx >= list.len() {
            return ScanErrorCode::InvalidPath as i32;
        }

        let app = &list[idx];
        *out_info = AppInfoFfi {
            kind: app.kind as u32,
            estimated_size: app.estimated_size,
            actual_size: app.actual_size,
            file_count: app.file_count,
            is_system_component: if app.is_system_component { 1 } else { 0 },
        };

        ScanErrorCode::Ok as i32
    }))
}

/// Retrieves a string field for the application at `index`.
/// `field_id`: 0=id, 1=name, 2=version, 3=publisher, 4=install_location,
/// 5=uninstall_string, 6=quiet_uninstall_string, 7=install_date.
///
/// # Safety
/// `catalog` and `out_actual_len` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn app_catalog_string(
    catalog: *const AppCatalogHandle,
    index: u32,
    field_id: u32,
    out_buffer: *mut u16,
    buffer_len: u32,
    out_actual_len: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if catalog.is_null() || out_actual_len.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let list = &(*catalog).0;
        let idx = index as usize;
        if idx >= list.len() {
            return ScanErrorCode::InvalidPath as i32;
        }

        let app = &list[idx];
        let val = match field_id {
            0 => &app.id,
            1 => &app.name,
            2 => &app.version,
            3 => &app.publisher,
            4 => &app.install_location,
            5 => &app.uninstall_string,
            6 => &app.quiet_uninstall_string,
            7 => &app.install_date,
            _ => return ScanErrorCode::InvalidPath as i32,
        };

        copy_str_to_wide_buf(val, out_buffer, buffer_len, out_actual_len)
    }))
}

/// Frees an `AppCatalogHandle`. Safe to call with null.
///
/// # Safety
/// `catalog` must be null or a valid pointer from `app_catalog_create`.
#[no_mangle]
pub unsafe extern "C" fn app_catalog_destroy(catalog: *mut AppCatalogHandle) {
    catch_ffi_panic((), std::panic::AssertUnwindSafe(|| {
        if !catalog.is_null() {
            drop(Box::from_raw(catalog));
        }
    }))
}

/// Detects orphaned application leftovers (ADR-014 §4).
///
/// # Safety
/// `out_leftovers` must be a valid, writable pointer.
#[no_mangle]
pub unsafe extern "C" fn app_leftovers_detect(
    catalog: *const AppCatalogHandle,
    tree_handle: *const TreeHandle,
    out_leftovers: *mut *mut AppLeftoversHandle,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if out_leftovers.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        *out_leftovers = std::ptr::null_mut();

        let empty_apps = Vec::new();
        let installed_apps = if !catalog.is_null() {
            &(*catalog).0
        } else {
            &empty_apps
        };

        let tree = if !tree_handle.is_null() {
            Some(&(*tree_handle).0)
        } else {
            None
        };

        let leftovers = storage_tree::app_manager::find_app_leftovers(installed_apps, tree);
        let boxed = Box::new(AppLeftoversHandle(leftovers));
        *out_leftovers = Box::into_raw(boxed);

        ScanErrorCode::Ok as i32
    }))
}

/// Returns the number of leftovers found.
///
/// # Safety
/// `leftovers` must be null or a valid pointer from `app_leftovers_detect`.
#[no_mangle]
pub unsafe extern "C" fn app_leftovers_count(leftovers: *const AppLeftoversHandle) -> u32 {
    catch_ffi_panic(0u32, std::panic::AssertUnwindSafe(|| {
        if leftovers.is_null() {
            0u32
        } else {
            (*leftovers).0.len() as u32
        }
    }))
}

/// Retrieves metadata numbers for the leftover at `index`.
///
/// # Safety
/// `leftovers` and `out_info` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn app_leftovers_item(
    leftovers: *const AppLeftoversHandle,
    index: u32,
    out_info: *mut AppLeftoverFfi,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if leftovers.is_null() || out_info.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let list = &(*leftovers).0;
        let idx = index as usize;
        if idx >= list.len() {
            return ScanErrorCode::InvalidPath as i32;
        }

        let l = &list[idx];
        *out_info = AppLeftoverFfi {
            size: l.size,
            file_count: l.file_count,
            location_type: l.location_type as u32,
            risk_level: l.risk_level as i32,
        };

        ScanErrorCode::Ok as i32
    }))
}

/// Retrieves string fields for the leftover at `index`.
/// `field_id`: 0=id, 1=app_name, 2=path.
///
/// # Safety
/// `leftovers` and `out_actual_len` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn app_leftovers_string(
    leftovers: *const AppLeftoversHandle,
    index: u32,
    field_id: u32,
    out_buffer: *mut u16,
    buffer_len: u32,
    out_actual_len: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if leftovers.is_null() || out_actual_len.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let list = &(*leftovers).0;
        let idx = index as usize;
        if idx >= list.len() {
            return ScanErrorCode::InvalidPath as i32;
        }

        let l = &list[idx];
        let path_str = l.path.to_string_lossy();
        let val = match field_id {
            0 => &l.id,
            1 => &l.app_name,
            2 => path_str.as_ref(),
            _ => return ScanErrorCode::InvalidPath as i32,
        };

        copy_str_to_wide_buf(val, out_buffer, buffer_len, out_actual_len)
    }))
}

/// Frees an `AppLeftoversHandle`. Safe to call with null.
///
/// # Safety
/// `leftovers` must be null or a valid pointer from `app_leftovers_detect`.
#[no_mangle]
pub unsafe extern "C" fn app_leftovers_destroy(leftovers: *mut AppLeftoversHandle) {
    catch_ffi_panic((), std::panic::AssertUnwindSafe(|| {
        if !leftovers.is_null() {
            drop(Box::from_raw(leftovers));
        }
    }))
}

// ---------------------------------------------------------------------------
// Phase 8: Developer Storage Intelligence & Safe Reclamation FFI (ADR-015)
// ---------------------------------------------------------------------------

/// Opaque handle wrapping discovered developer artifacts (ADR-015).
pub struct DevCatalogHandle(pub Vec<storage_tree::DevArtifact>);

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevArtifactFfi {
    pub ecosystem: u32,
    pub kind: u32,
    pub size: u64,
    pub file_count: u64,
    pub is_dormant: u32,
    pub days_inactive: u32,
    pub risk_level: i32,
}

/// Discovers developer artifacts across the active StorageTree and canonical Windows dev paths (ADR-015).
/// `tree_handle` is optional (may be null).
/// `*out_catalog` receives an owned handle the caller must destroy via `dev_catalog_destroy`.
///
/// # Safety
/// `out_catalog` must be a valid, writable pointer.
#[no_mangle]
pub unsafe extern "C" fn dev_catalog_create(
    tree_handle: *const TreeHandle,
    out_catalog: *mut *mut DevCatalogHandle,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if out_catalog.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        *out_catalog = std::ptr::null_mut();

        let tree = if !tree_handle.is_null() {
            Some(&(*tree_handle).0)
        } else {
            None
        };

        let artifacts = storage_tree::detect_dev_artifacts(tree);
        let boxed = Box::new(DevCatalogHandle(artifacts));
        *out_catalog = Box::into_raw(boxed);

        ScanErrorCode::Ok as i32
    }))
}

/// Returns the number of developer artifacts discovered.
///
/// # Safety
/// `catalog` must be null or a valid pointer from `dev_catalog_create`.
#[no_mangle]
pub unsafe extern "C" fn dev_catalog_count(catalog: *const DevCatalogHandle) -> u32 {
    catch_ffi_panic(0u32, std::panic::AssertUnwindSafe(|| {
        if catalog.is_null() {
            0u32
        } else {
            (*catalog).0.len() as u32
        }
    }))
}

/// Retrieves numeric metadata for the developer artifact at `index`.
///
/// # Safety
/// `catalog` and `out_info` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn dev_catalog_item(
    catalog: *const DevCatalogHandle,
    index: u32,
    out_info: *mut DevArtifactFfi,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if catalog.is_null() || out_info.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let list = &(*catalog).0;
        let idx = index as usize;
        if idx >= list.len() {
            return ScanErrorCode::InvalidPath as i32;
        }

        let item = &list[idx];
        *out_info = DevArtifactFfi {
            ecosystem: item.ecosystem as u32,
            kind: item.kind as u32,
            size: item.size,
            file_count: item.file_count,
            is_dormant: if item.is_dormant { 1 } else { 0 },
            days_inactive: item.days_inactive,
            risk_level: item.risk_level as i32,
        };

        ScanErrorCode::Ok as i32
    }))
}

/// Retrieves a string field for the developer artifact at `index`.
/// `field_id`: 0=id, 1=name, 2=project_name, 3=path, 4=description, 5=cleanup_command.
///
/// # Safety
/// `catalog` and `out_actual_len` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn dev_catalog_string(
    catalog: *const DevCatalogHandle,
    index: u32,
    field_id: u32,
    out_buffer: *mut u16,
    buffer_len: u32,
    out_actual_len: *mut u32,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if catalog.is_null() || out_actual_len.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        let list = &(*catalog).0;
        let idx = index as usize;
        if idx >= list.len() {
            return ScanErrorCode::InvalidPath as i32;
        }

        let item = &list[idx];
        let path_str = item.path.to_string_lossy();
        let val = match field_id {
            0 => &item.id,
            1 => &item.name,
            2 => &item.project_name,
            3 => path_str.as_ref(),
            4 => &item.description,
            5 => &item.cleanup_command,
            _ => return ScanErrorCode::InvalidPath as i32,
        };

        copy_str_to_wide_buf(val, out_buffer, buffer_len, out_actual_len)
    }))
}

/// Frees a `DevCatalogHandle`. Safe to call with null.
///
/// # Safety
/// `catalog` must be null or a valid pointer from `dev_catalog_create`.
#[no_mangle]
pub unsafe extern "C" fn dev_catalog_destroy(catalog: *mut DevCatalogHandle) {
    catch_ffi_panic((), std::panic::AssertUnwindSafe(|| {
        if !catalog.is_null() {
            drop(Box::from_raw(catalog));
        }
    }))
}

/// Safely cleans a developer artifact directory or cache (ADR-015 §4).
/// Enforces zero-accident guardrails (source files and git roots are rejected).
///
/// # Safety
/// `path` and `out_report` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn dev_clean_artifact(
    path: *const u16,
    dry_run: u8,
    send_to_recycle_bin: u8,
    out_report: *mut CleanupReportFfi,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if path.is_null() || out_report.is_null() {
            return ScanErrorCode::Internal as i32;
        }

        let path_buf = match wide_ptr_to_pathbuf(path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };

        let report = storage_tree::clean_dev_artifact(
            &path_buf,
            dry_run != 0,
            send_to_recycle_bin != 0,
        );

        *out_report = CleanupReportFfi {
            files_reclaimed: report.files_reclaimed,
            bytes_reclaimed: report.bytes_reclaimed,
            files_failed: report.files_failed,
            is_dry_run: if report.is_dry_run { 1 } else { 0 },
        };

        ScanErrorCode::Ok as i32
    }))
}

// ---------------------------------------------------------------------------
// Phase 9: Incremental Indexing & SQLite Persistence FFI (ADR-016)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexStatsFfi {
    pub volume_id: u64,
    pub node_count: u64,
    pub total_size: u64,
    pub last_scan_time: u64,
    pub last_usn: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexSyncReportFfi {
    pub nodes_added: u64,
    pub nodes_updated: u64,
    pub nodes_removed: u64,
    pub bytes_delta: i64,
    pub sync_duration_ms: u64,
}

/// Persists an in-memory `StorageTree` hierarchy into the SQLite index (ADR-016).
///
/// # Safety
/// `db_path`, `tree_handle`, and `root_path` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn index_save_tree(
    db_path: *const u16,
    tree_handle: *const TreeHandle,
    root_path: *const u16,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if db_path.is_null() || tree_handle.is_null() || root_path.is_null() {
            return ScanErrorCode::Internal as i32;
        }

        let db_buf = match wide_ptr_to_pathbuf(db_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };
        let root_buf = match wide_ptr_to_pathbuf(root_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };

        let tree = &(*tree_handle).0;
        match storage_tree::save_tree_to_db(&db_buf, tree, &root_buf) {
            Ok(()) => ScanErrorCode::Ok as i32,
            Err(_) => ScanErrorCode::Internal as i32,
        }
    }))
}

/// Hydrates an in-memory `StorageTree` directly from the SQLite index in < 100ms (ADR-016).
///
/// # Safety
/// `db_path`, `root_path`, and `out_tree` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn index_load_tree(
    db_path: *const u16,
    root_path: *const u16,
    out_tree: *mut *mut TreeHandle,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if db_path.is_null() || root_path.is_null() || out_tree.is_null() {
            return ScanErrorCode::Internal as i32;
        }
        *out_tree = std::ptr::null_mut();

        let db_buf = match wide_ptr_to_pathbuf(db_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };
        let root_buf = match wide_ptr_to_pathbuf(root_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };

        match storage_tree::load_tree_from_db(&db_buf, &root_buf) {
            Ok(Some(tree)) => {
                let boxed = Box::new(TreeHandle(tree));
                *out_tree = Box::into_raw(boxed);
                ScanErrorCode::Ok as i32
            }
            Ok(None) => ScanErrorCode::InvalidPath as i32,
            Err(_) => ScanErrorCode::Internal as i32,
        }
    }))
}

/// Incrementally synchronizes filesystem changes into the tree and database (ADR-016).
///
/// # Safety
/// `db_path`, `tree_handle`, `root_path`, and `out_report` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn index_sync_tree(
    db_path: *const u16,
    tree_handle: *mut TreeHandle,
    root_path: *const u16,
    out_report: *mut IndexSyncReportFfi,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if db_path.is_null() || tree_handle.is_null() || root_path.is_null() || out_report.is_null() {
            return ScanErrorCode::Internal as i32;
        }

        let db_buf = match wide_ptr_to_pathbuf(db_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };
        let root_buf = match wide_ptr_to_pathbuf(root_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };

        let tree = &mut (*tree_handle).0;
        match storage_tree::sync_tree_incremental(&db_buf, tree, &root_buf) {
            Ok(report) => {
                *out_report = IndexSyncReportFfi {
                    nodes_added: report.nodes_added,
                    nodes_updated: report.nodes_updated,
                    nodes_removed: report.nodes_removed,
                    bytes_delta: report.bytes_delta,
                    sync_duration_ms: report.sync_duration_ms,
                };
                ScanErrorCode::Ok as i32
            }
            Err(_) => ScanErrorCode::Internal as i32,
        }
    }))
}

/// Retrieves indexed volume metrics (ADR-016).
///
/// # Safety
/// `db_path`, `root_path`, and `out_stats` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn index_get_stats(
    db_path: *const u16,
    root_path: *const u16,
    out_stats: *mut IndexStatsFfi,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if db_path.is_null() || root_path.is_null() || out_stats.is_null() {
            return ScanErrorCode::Internal as i32;
        }

        let db_buf = match wide_ptr_to_pathbuf(db_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };
        let root_buf = match wide_ptr_to_pathbuf(root_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };

        match storage_tree::get_index_stats(&db_buf, &root_buf) {
            Ok(Some(stats)) => {
                *out_stats = IndexStatsFfi {
                    volume_id: stats.volume_id,
                    node_count: stats.node_count,
                    total_size: stats.total_size,
                    last_scan_time: stats.last_scan_time,
                    last_usn: stats.last_usn,
                };
                ScanErrorCode::Ok as i32
            }
            Ok(None) => ScanErrorCode::InvalidPath as i32,
            Err(_) => ScanErrorCode::Internal as i32,
        }
    }))
}

/// Deletes an indexed volume from SQLite (ADR-016).
///
/// # Safety
/// `db_path`, `root_path`, and `out_deleted` must be valid non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn index_delete_volume(
    db_path: *const u16,
    root_path: *const u16,
    out_deleted: *mut u8,
) -> i32 {
    catch_ffi_panic(ScanErrorCode::Internal as i32, std::panic::AssertUnwindSafe(|| {
        if db_path.is_null() || root_path.is_null() || out_deleted.is_null() {
            return ScanErrorCode::Internal as i32;
        }

        let db_buf = match wide_ptr_to_pathbuf(db_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };
        let root_buf = match wide_ptr_to_pathbuf(root_path) {
            Some(p) => p,
            None => return ScanErrorCode::InvalidPath as i32,
        };

        match storage_tree::delete_indexed_volume(&db_buf, &root_buf) {
            Ok(deleted) => {
                *out_deleted = if deleted { 1 } else { 0 };
                ScanErrorCode::Ok as i32
            }
            Err(_) => ScanErrorCode::Internal as i32,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn panic_at_ffi_boundary_is_caught_not_propagated() {
        let result = catch_ffi_panic(false, || {
            panic!("simulated internal bug");
            #[allow(unreachable_code)]
            true
        });
        assert!(!result); // default value returned, no unwind escaped this test
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
        let missing: Vec<u16> = "Z:\\this_should_not_exist_ffi_test\0".encode_utf16().collect();
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

    // -----------------------------------------------------------------------
    // Phase 3: Tree FFI integration tests
    // -----------------------------------------------------------------------

    /// Helper: scan a temp directory and return the raw ScanResultHandle.
    unsafe fn scan_temp_dir(dir: &std::path::Path) -> *mut ScanResultHandle {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let cancel = cancel_token_create();
        let mut result: *mut ScanResultHandle = std::ptr::null_mut();
        let mut err_msg: *mut u16 = std::ptr::null_mut();

        let code = scan_drive(wide.as_ptr(), cancel, None, std::ptr::null_mut(), &mut result, &mut err_msg);
        assert_eq!(code, ScanErrorCode::Ok as i32, "scan_drive failed");
        assert!(!result.is_null());
        cancel_token_destroy(cancel);
        result
    }

    #[test]
    fn tree_lifecycle_create_query_destroy() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("big.bin"), vec![b'x'; 2000]).unwrap();
        std::fs::write(dir.path().join("small.txt"), b"hi").unwrap();

        unsafe {
            let scan_result = scan_temp_dir(dir.path());

            // Create tree from scan result
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            let code = tree_create(scan_result, &mut tree, &mut err_msg);
            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert!(!tree.is_null());
            assert!(err_msg.is_null());

            // Root id
            let root_id = tree_root_id(tree);
            assert_ne!(root_id, u64::MAX);

            // Node count: root + 2 files
            let count = tree_node_count(tree);
            assert_eq!(count, 3);

            // Root node info
            let mut info = std::mem::zeroed::<NodeInfoFfi>();
            let code = tree_node_info(tree, root_id, &mut info);
            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert_eq!(info.kind, 0); // Directory

            // Children (should be 2, sorted by size descending)
            let mut child_ids = [0u64; 10];
            let mut out_count: u32 = 0;
            let mut out_total: u32 = 0;
            let code = tree_children(tree, root_id, 0, 10, child_ids.as_mut_ptr(), &mut out_count, &mut out_total);
            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert_eq!(out_total, 2);
            assert_eq!(out_count, 2);

            // First child should be the larger file
            let mut first_info = std::mem::zeroed::<NodeInfoFfi>();
            tree_node_info(tree, child_ids[0], &mut first_info);
            assert!(first_info.size >= 2000); // big.bin (on-disk, at least 2000)

            // Cleanup
            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn tree_children_pagination() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        for i in 0..20u32 {
            std::fs::write(
                dir.path().join(format!("file{i:02}.bin")),
                vec![b'x'; (i as usize + 1) * 100],
            ).unwrap();
        }

        unsafe {
            let scan_result = scan_temp_dir(dir.path());
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            tree_create(scan_result, &mut tree, &mut err_msg);

            // Request page: offset=5, limit=5
            let mut ids = [0u64; 5];
            let mut count: u32 = 0;
            let mut total: u32 = 0;
            let root = tree_root_id(tree);
            tree_children(tree, root, 5, 5, ids.as_mut_ptr(), &mut count, &mut total);
            assert_eq!(total, 20);
            assert_eq!(count, 5);

            // Request beyond end: offset=18, limit=5 → only 2 results
            tree_children(tree, root, 18, 5, ids.as_mut_ptr(), &mut count, &mut total);
            assert_eq!(total, 20);
            assert_eq!(count, 2);

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn tree_node_info_invalid_id_returns_error() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), b"data").unwrap();

        unsafe {
            let scan_result = scan_temp_dir(dir.path());
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            tree_create(scan_result, &mut tree, &mut err_msg);

            let mut info = std::mem::zeroed::<NodeInfoFfi>();
            let code = tree_node_info(tree, 99999, &mut info);
            assert_ne!(code, ScanErrorCode::Ok as i32);

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn tree_create_twice_returns_error() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), b"data").unwrap();

        unsafe {
            let scan_result = scan_temp_dir(dir.path());

            let mut tree1: *mut TreeHandle = std::ptr::null_mut();
            let mut err1: *mut u16 = std::ptr::null_mut();
            let code1 = tree_create(scan_result, &mut tree1, &mut err1);
            assert_eq!(code1, ScanErrorCode::Ok as i32);

            // Second tree_create on same result should fail (events consumed)
            let mut tree2: *mut TreeHandle = std::ptr::null_mut();
            let mut err2: *mut u16 = std::ptr::null_mut();
            let code2 = tree_create(scan_result, &mut tree2, &mut err2);
            assert_eq!(code2, ScanErrorCode::Internal as i32);
            assert!(tree2.is_null());
            if !err2.is_null() { free_error_message(err2); }

            tree_destroy(tree1);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn tree_destroy_null_is_noop() {
        unsafe {
            tree_destroy(std::ptr::null_mut()); // should not crash
        }
    }

    #[test]
    fn tree_ancestors_on_root_returns_just_root() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), b"data").unwrap();

        unsafe {
            let scan_result = scan_temp_dir(dir.path());
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            tree_create(scan_result, &mut tree, &mut err_msg);

            let root = tree_root_id(tree);
            let mut ids = [0u64; 10];
            let mut count: u32 = 0;
            tree_ancestors(tree, root, ids.as_mut_ptr(), 10, &mut count);
            assert_eq!(count, 1);
            assert_eq!(ids[0], root);

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn tree_top_files_by_size_via_ffi() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("small.txt"), b"hi").unwrap();
        std::fs::write(dir.path().join("big.bin"), vec![b'x'; 5000]).unwrap();
        std::fs::write(dir.path().join("medium.dat"), vec![b'y'; 500]).unwrap();

        unsafe {
            let scan_result = scan_temp_dir(dir.path());
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            tree_create(scan_result, &mut tree, &mut err_msg);

            let root = tree_root_id(tree);
            let mut ids = [0u64; 2];
            let mut count: u32 = 0;
            tree_top_files_by_size(tree, root, 2, ids.as_mut_ptr(), &mut count);
            assert_eq!(count, 2);

            // Verify they're in descending size order
            let mut info1 = std::mem::zeroed::<NodeInfoFfi>();
            let mut info2 = std::mem::zeroed::<NodeInfoFfi>();
            tree_node_info(tree, ids[0], &mut info1);
            tree_node_info(tree, ids[1], &mut info2);
            assert!(info1.size >= info2.size);

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn tree_node_name_and_path_via_ffi() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub_dir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("hello.txt"), b"world").unwrap();

        unsafe {
            let scan_result = scan_temp_dir(dir.path());
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            tree_create(scan_result, &mut tree, &mut err_msg);

            let root = tree_root_id(tree);
            let mut name_buf = [0u16; 64];
            let mut actual_len = 0u32;
            let code = tree_node_name(tree, root, name_buf.as_mut_ptr(), 64, &mut actual_len);
            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert!(actual_len > 0);

            let mut path_buf = [0u16; 260];
            let mut path_len = 0u32;
            let path_code = tree_node_path(tree, root, path_buf.as_mut_ptr(), 260, &mut path_len);
            assert_eq!(path_code, ScanErrorCode::Ok as i32);
            assert!(path_len > 0);

            // Test children name and path
            let mut child_ids = [0u64; 10];
            let mut child_count = 0u32;
            let mut total_children = 0u32;
            tree_children(tree, root, 0, 10, child_ids.as_mut_ptr(), &mut child_count, &mut total_children);
            assert_eq!(child_count, 1);

            let mut sub_name_buf = [0u16; 64];
            let mut sub_name_len = 0u32;
            tree_node_name(tree, child_ids[0], sub_name_buf.as_mut_ptr(), 64, &mut sub_name_len);
            let sub_name = String::from_utf16_lossy(&sub_name_buf[..sub_name_len as usize]);
            assert_eq!(sub_name, "sub_dir");

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn tree_compute_layout_via_ffi() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("video.mp4"), vec![b'v'; 10000]).unwrap();
        std::fs::write(dir.path().join("photo.jpg"), vec![b'p'; 5000]).unwrap();

        unsafe {
            let scan_result = scan_temp_dir(dir.path());
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            tree_create(scan_result, &mut tree, &mut err_msg);

            let root = tree_root_id(tree);
            let mut rects = [std::mem::zeroed::<TreemapRectFfi>(); 10];
            let mut count = 0u32;
            let mut total = 0u32;

            let code = tree_compute_layout(
                tree,
                root,
                1000.0,
                800.0,
                2,
                2.0,
                rects.as_mut_ptr(),
                10,
                &mut count,
                &mut total,
            );

            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert!(total >= 2);
            assert!(count >= 2);

            for i in 0..count as usize {
                assert!(rects[i].width > 0.0);
                assert!(rects[i].height > 0.0);
                assert!(rects[i].x >= 0.0 && rects[i].x + rects[i].width <= 1000.5);
                assert!(rects[i].y >= 0.0 && rects[i].y + rects[i].height <= 800.5);
            }

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn tree_search_via_ffi() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("docker_vm.vhdx"), vec![b'x'; 20000]).unwrap();
        std::fs::write(dir.path().join("readme.txt"), vec![b'r'; 1000]).unwrap();

        unsafe {
            let scan_result = scan_temp_dir(dir.path());
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            tree_create(scan_result, &mut tree, &mut err_msg);

            let query_str: Vec<u16> = "ext:vhdx".encode_utf16().chain(std::iter::once(0)).collect();
            let mut results = [std::mem::zeroed::<SearchResultFfi>(); 10];
            let mut count = 0u32;

            let code = tree_search(
                tree,
                query_str.as_ptr(),
                10,
                results.as_mut_ptr(),
                &mut count,
            );

            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert_eq!(count, 1);
            assert_eq!(results[0].size, 20480);
            assert_eq!(results[0].category, storage_tree::FileCategory::Other as i32);

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn cleanup_ffi_roundtrip() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("crash.dmp"), vec![b'c'; 8000]).unwrap();

        unsafe {
            let scan_result = scan_temp_dir(dir.path());
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            tree_create(scan_result, &mut tree, &mut err_msg);

            let mut candidates = [std::mem::zeroed::<CleanupCandidateFfi>(); 10];
            let mut count = 0u32;
            let mut total = 0u32;

            let code = cleanup_detect_candidates(
                tree,
                candidates.as_mut_ptr(),
                10,
                &mut count,
                &mut total,
            );

            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert!(total >= 7);
            assert!(count >= 7);

            // Execute dry run on crash dumps (rule_id = 3)
            let mut report = std::mem::zeroed::<CleanupReportFfi>();
            let exec_code = cleanup_execute_rule(
                tree,
                3, // CrashDumps
                1, // dry_run = true
                1, // send_to_recycle_bin = true
                &mut report,
            );

            assert_eq!(exec_code, ScanErrorCode::Ok as i32);
            assert_eq!(report.is_dry_run, 1);
            assert!(report.files_reclaimed >= 1);

            // Verify is_path_protected helper via FFI
            let sys_path: Vec<u16> = "C:\\Windows\\System32".encode_utf16().chain(std::iter::once(0)).collect();
            assert_eq!(cleanup_is_path_protected(sys_path.as_ptr()), 1);

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }

    #[test]
    fn app_catalog_and_leftovers_ffi_roundtrip() {
        unsafe {
            let mut catalog: *mut AppCatalogHandle = std::ptr::null_mut();
            let code = app_catalog_create(std::ptr::null(), &mut catalog);
            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert!(!catalog.is_null());

            let count = app_catalog_count(catalog);
            if count > 0 {
                let mut info = std::mem::zeroed::<AppInfoFfi>();
                let item_code = app_catalog_item(catalog, 0, &mut info);
                assert_eq!(item_code, ScanErrorCode::Ok as i32);

                let mut name_buf = [0u16; 256];
                let mut actual_len = 0u32;
                let str_code = app_catalog_string(
                    catalog,
                    0,
                    1, // name
                    name_buf.as_mut_ptr(),
                    name_buf.len() as u32,
                    &mut actual_len,
                );
                assert_eq!(str_code, ScanErrorCode::Ok as i32);
                assert!(actual_len > 0);
            }

            // Leftover detection
            let mut leftovers: *mut AppLeftoversHandle = std::ptr::null_mut();
            let l_code = app_leftovers_detect(catalog, std::ptr::null(), &mut leftovers);
            assert_eq!(l_code, ScanErrorCode::Ok as i32);
            assert!(!leftovers.is_null());

            let l_count = app_leftovers_count(leftovers);
            if l_count > 0 {
                let mut l_info = std::mem::zeroed::<AppLeftoverFfi>();
                let l_item_code = app_leftovers_item(leftovers, 0, &mut l_info);
                assert_eq!(l_item_code, ScanErrorCode::Ok as i32);
            }

            app_leftovers_destroy(leftovers);
            app_catalog_destroy(catalog);
        }
    }

    #[test]
    fn dev_catalog_ffi_roundtrip() {
        unsafe {
            let mut catalog: *mut DevCatalogHandle = std::ptr::null_mut();
            let code = dev_catalog_create(std::ptr::null(), &mut catalog);
            assert_eq!(code, ScanErrorCode::Ok as i32);
            assert!(!catalog.is_null());

            let count = dev_catalog_count(catalog);
            if count > 0 {
                let mut info = std::mem::zeroed::<DevArtifactFfi>();
                let item_code = dev_catalog_item(catalog, 0, &mut info);
                assert_eq!(item_code, ScanErrorCode::Ok as i32);

                let mut name_buf = [0u16; 256];
                let mut actual_len = 0u32;
                let str_code = dev_catalog_string(
                    catalog,
                    0,
                    1, // name
                    name_buf.as_mut_ptr(),
                    name_buf.len() as u32,
                    &mut actual_len,
                );
                assert_eq!(str_code, ScanErrorCode::Ok as i32);
                assert!(actual_len > 0);
            }

            // Test cleaning safety guardrail via dev_clean_artifact
            let git_path: Vec<u16> = "C:\\Repo\\.git".encode_utf16().chain(std::iter::once(0)).collect();
            let mut report = std::mem::zeroed::<CleanupReportFfi>();
            let clean_code = dev_clean_artifact(git_path.as_ptr(), 1, 1, &mut report);
            assert_eq!(clean_code, ScanErrorCode::Ok as i32);
            assert_eq!(report.files_failed, 1);
            assert_eq!(report.files_reclaimed, 0);

            dev_catalog_destroy(catalog);
        }
    }

    #[test]
    fn index_ffi_roundtrip() {
        use std::os::windows::ffi::OsStrExt;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let test_dir = dir.path().join("fixture");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::fs::write(test_dir.join("test.bin"), vec![b'a'; 1024]).unwrap();

        let db_path = dir.path().join("index.db");
        let db_wide: Vec<u16> = db_path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let root_wide: Vec<u16> = test_dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();

        unsafe {
            let scan_result = scan_temp_dir(&test_dir);
            let mut tree: *mut TreeHandle = std::ptr::null_mut();
            let mut err_msg: *mut u16 = std::ptr::null_mut();
            tree_create(scan_result, &mut tree, &mut err_msg);
            assert!(!tree.is_null());

            // 1. Save tree
            let save_code = index_save_tree(db_wide.as_ptr(), tree, root_wide.as_ptr());
            assert_eq!(save_code, ScanErrorCode::Ok as i32);

            // 2. Get stats
            let mut stats = std::mem::zeroed::<IndexStatsFfi>();
            let stats_code = index_get_stats(db_wide.as_ptr(), root_wide.as_ptr(), &mut stats);
            assert_eq!(stats_code, ScanErrorCode::Ok as i32);
            assert!(stats.node_count >= 2);
            assert!(stats.total_size >= 1024);

            // 3. Load tree
            let mut loaded_tree: *mut TreeHandle = std::ptr::null_mut();
            let load_code = index_load_tree(db_wide.as_ptr(), root_wide.as_ptr(), &mut loaded_tree);
            assert_eq!(load_code, ScanErrorCode::Ok as i32);
            assert!(!loaded_tree.is_null());
            assert_eq!(tree_node_count(loaded_tree), tree_node_count(tree));

            // 4. Sync tree
            let mut sync_rep = std::mem::zeroed::<IndexSyncReportFfi>();
            let sync_code = index_sync_tree(db_wide.as_ptr(), loaded_tree, root_wide.as_ptr(), &mut sync_rep);
            assert_eq!(sync_code, ScanErrorCode::Ok as i32);

            // 5. Delete volume
            let mut deleted = 0u8;
            let del_code = index_delete_volume(db_wide.as_ptr(), root_wide.as_ptr(), &mut deleted);
            assert_eq!(del_code, ScanErrorCode::Ok as i32);
            assert_eq!(deleted, 1);

            tree_destroy(loaded_tree);
            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }
}

