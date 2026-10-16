//! Incremental SQLite index persistence exports (ADR-016).

use crate::error::{catch_ffi_panic, ScanErrorCode};
use crate::handles::TreeHandle;
use crate::marshal::wide_ptr_to_pathbuf;

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
    use crate::*;

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
