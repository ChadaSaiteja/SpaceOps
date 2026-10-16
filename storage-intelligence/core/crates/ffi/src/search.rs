//! Search export (ADR-012).

use crate::error::{catch_ffi_panic, ScanErrorCode};
use crate::handles::TreeHandle;
use crate::marshal::wide_ptr_to_string;
use crate::tree::node_kind_to_i32;

/// C-compatible representation of a search result hit (ADR-012).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchResultFfi {
    pub node_id: u64,
    pub size: u64,
    pub kind: i32,     // 0=Dir, 1=File, 2=ReparsePoint, 3=Inaccessible
    pub category: i32, // 0=Video, 1=Audio, 2=Image, 3=Document, 4=Archive, 5=Executable, 6=Code, 7=System, 8=Other
    pub score: u32,
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::scan_temp_dir;
    use crate::*;

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

            let query_str: Vec<u16> = "ext:vhdx"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
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
            assert_eq!(
                results[0].category,
                storage_tree::FileCategory::Other as i32
            );

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }
}
