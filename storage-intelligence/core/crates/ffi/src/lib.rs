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

mod cleanup;
mod error;
mod handles;
mod marshal;
mod scan;
mod search;
#[cfg(test)]
mod test_util;
mod tree;
mod treemap;

pub use cleanup::*;
pub use error::*;
pub use handles::*;
pub(crate) use marshal::*;
pub use scan::*;
pub use search::*;
pub use tree::*;
pub use treemap::*;

// ---------------------------------------------------------------------------
// Phase 3: Storage Tree FFI exports (ADR-009)
// ---------------------------------------------------------------------------

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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
}

/// Returns the number of applications in the catalog.
///
/// # Safety
/// `catalog` must be null or a valid pointer from `app_catalog_create`.
#[no_mangle]
pub unsafe extern "C" fn app_catalog_count(catalog: *const AppCatalogHandle) -> u32 {
    catch_ffi_panic(
        0u32,
        std::panic::AssertUnwindSafe(|| {
            if catalog.is_null() {
                0u32
            } else {
                (*catalog).0.len() as u32
            }
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
}

/// Frees an `AppCatalogHandle`. Safe to call with null.
///
/// # Safety
/// `catalog` must be null or a valid pointer from `app_catalog_create`.
#[no_mangle]
pub unsafe extern "C" fn app_catalog_destroy(catalog: *mut AppCatalogHandle) {
    catch_ffi_panic(
        (),
        std::panic::AssertUnwindSafe(|| {
            if !catalog.is_null() {
                drop(Box::from_raw(catalog));
            }
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
}

/// Returns the number of leftovers found.
///
/// # Safety
/// `leftovers` must be null or a valid pointer from `app_leftovers_detect`.
#[no_mangle]
pub unsafe extern "C" fn app_leftovers_count(leftovers: *const AppLeftoversHandle) -> u32 {
    catch_ffi_panic(
        0u32,
        std::panic::AssertUnwindSafe(|| {
            if leftovers.is_null() {
                0u32
            } else {
                (*leftovers).0.len() as u32
            }
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
}

/// Frees an `AppLeftoversHandle`. Safe to call with null.
///
/// # Safety
/// `leftovers` must be null or a valid pointer from `app_leftovers_detect`.
#[no_mangle]
pub unsafe extern "C" fn app_leftovers_destroy(leftovers: *mut AppLeftoversHandle) {
    catch_ffi_panic(
        (),
        std::panic::AssertUnwindSafe(|| {
            if !leftovers.is_null() {
                drop(Box::from_raw(leftovers));
            }
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
}

/// Returns the number of developer artifacts discovered.
///
/// # Safety
/// `catalog` must be null or a valid pointer from `dev_catalog_create`.
#[no_mangle]
pub unsafe extern "C" fn dev_catalog_count(catalog: *const DevCatalogHandle) -> u32 {
    catch_ffi_panic(
        0u32,
        std::panic::AssertUnwindSafe(|| {
            if catalog.is_null() {
                0u32
            } else {
                (*catalog).0.len() as u32
            }
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
}

/// Frees a `DevCatalogHandle`. Safe to call with null.
///
/// # Safety
/// `catalog` must be null or a valid pointer from `dev_catalog_create`.
#[no_mangle]
pub unsafe extern "C" fn dev_catalog_destroy(catalog: *mut DevCatalogHandle) {
    catch_ffi_panic(
        (),
        std::panic::AssertUnwindSafe(|| {
            if !catalog.is_null() {
                drop(Box::from_raw(catalog));
            }
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
            if path.is_null() || out_report.is_null() {
                return ScanErrorCode::Internal as i32;
            }

            let path_buf = match wide_ptr_to_pathbuf(path) {
                Some(p) => p,
                None => return ScanErrorCode::InvalidPath as i32,
            };

            let report =
                storage_tree::clean_dev_artifact(&path_buf, dry_run != 0, send_to_recycle_bin != 0);

            *out_report = CleanupReportFfi {
                files_reclaimed: report.files_reclaimed,
                bytes_reclaimed: report.bytes_reclaimed,
                files_failed: report.files_failed,
                is_dry_run: if report.is_dry_run { 1 } else { 0 },
            };

            ScanErrorCode::Ok as i32
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
            if db_path.is_null()
                || tree_handle.is_null()
                || root_path.is_null()
                || out_report.is_null()
            {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::scan_temp_dir;

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
            let git_path: Vec<u16> = "C:\\Repo\\.git"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
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
        let db_wide: Vec<u16> = db_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let root_wide: Vec<u16> = test_dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

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
            let sync_code = index_sync_tree(
                db_wide.as_ptr(),
                loaded_tree,
                root_wide.as_ptr(),
                &mut sync_rep,
            );
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
