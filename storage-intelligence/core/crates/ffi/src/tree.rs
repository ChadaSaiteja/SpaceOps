//! Storage tree lifecycle and node queries (ADR-009).

use crate::error::{catch_ffi_panic, init_logging_impl_once, set_error_message, ScanErrorCode};
use crate::handles::{ScanResultHandle, TreeHandle};
use crate::marshal::wide_ptr_to_pathbuf;

/// C-compatible node info struct. Data is copied from the tree into this struct;
/// string pointers (`name`, `extension`) point into caller-provided buffers or
/// are encoded inline as UTF-16. For simplicity at MVP, names are copied into
/// a heap-allocated UTF-16 buffer that lives until `tree_destroy`.
#[repr(C)]
pub struct NodeInfoFfi {
    pub id: u64,
    pub parent_id: u64, // u64::MAX if root (no parent)
    pub kind: i32,      // 0=Dir, 1=File, 2=ReparsePoint, 3=Inaccessible
    pub size: u64,
    pub file_count: u64,
    pub dir_count: u64,
}

pub(crate) fn node_kind_to_i32(kind: storage_tree::NodeKind) -> i32 {
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
                    set_error_message(
                        out_error_message,
                        "events already consumed by a previous tree_create call",
                    );
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
        }),
    )
}

#[no_mangle]
/// # Safety
/// `handle` must be null or a valid pointer from `tree_create`, not yet destroyed.
pub unsafe extern "C" fn tree_destroy(handle: *mut TreeHandle) {
    catch_ffi_panic(
        (),
        std::panic::AssertUnwindSafe(|| {
            if !handle.is_null() {
                drop(Box::from_raw(handle));
            }
        }),
    )
}

/// Returns the root node ID, or `u64::MAX` on error.
///
/// # Safety
/// `handle` must be a valid pointer returned by `tree_create`.
#[no_mangle]
pub unsafe extern "C" fn tree_root_id(handle: *const TreeHandle) -> u64 {
    catch_ffi_panic(
        u64::MAX,
        std::panic::AssertUnwindSafe(|| match handle.as_ref() {
            Some(h) => h.0.root().0,
            None => u64::MAX,
        }),
    )
}

/// Returns the total number of nodes in the tree, or 0 on error.
///
/// # Safety
/// `handle` must be a valid pointer returned by `tree_create`.
#[no_mangle]
pub unsafe extern "C" fn tree_node_count(handle: *const TreeHandle) -> u64 {
    catch_ffi_panic(
        0,
        std::panic::AssertUnwindSafe(|| match handle.as_ref() {
            Some(h) => h.0.node_count() as u64,
            None => 0,
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
            if handle.is_null()
                || out_ids.is_null()
                || out_count.is_null()
                || out_total_children.is_null()
            {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::scan_temp_dir;
    use crate::*;

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
            let code = tree_children(
                tree,
                root_id,
                0,
                10,
                child_ids.as_mut_ptr(),
                &mut out_count,
                &mut out_total,
            );
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
            )
            .unwrap();
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
            if !err2.is_null() {
                free_error_message(err2);
            }

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
            tree_children(
                tree,
                root,
                0,
                10,
                child_ids.as_mut_ptr(),
                &mut child_count,
                &mut total_children,
            );
            assert_eq!(child_count, 1);

            let mut sub_name_buf = [0u16; 64];
            let mut sub_name_len = 0u32;
            tree_node_name(
                tree,
                child_ids[0],
                sub_name_buf.as_mut_ptr(),
                64,
                &mut sub_name_len,
            );
            let sub_name = String::from_utf16_lossy(&sub_name_buf[..sub_name_len as usize]);
            assert_eq!(sub_name, "sub_dir");

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }
}
