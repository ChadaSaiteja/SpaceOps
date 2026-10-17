//! Cleanup candidate detection, execution and path protection (ADR-013).

use crate::error::{catch_ffi_panic, ScanErrorCode};
use crate::handles::TreeHandle;
use crate::marshal::wide_ptr_to_pathbuf;

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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
        }),
    )
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
    catch_ffi_panic(
        ScanErrorCode::Internal as i32,
        std::panic::AssertUnwindSafe(|| {
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
                Some(cand) => storage_tree::cleanup::execute_cleanup(
                    &cand.target_paths,
                    dry_run != 0,
                    send_to_recycle_bin != 0,
                ),
                None => storage_tree::CleanupReport::default(),
            };

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

/// Checks if a filesystem path is an unbypassable protected system location (ADR-013).
///
/// # Safety
/// `path` must be a null-terminated UTF-16 string pointer.
#[no_mangle]
pub unsafe extern "C" fn cleanup_is_path_protected(path: *const u16) -> u8 {
    catch_ffi_panic(
        1u8,
        std::panic::AssertUnwindSafe(|| {
            if path.is_null() {
                return 1u8;
            }
            match wide_ptr_to_pathbuf(path) {
                Some(p) => {
                    if storage_tree::is_path_protected(&p) {
                        1u8
                    } else {
                        0u8
                    }
                }
                None => 1u8,
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
            let sys_path: Vec<u16> = "C:\\Windows\\System32"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            assert_eq!(cleanup_is_path_protected(sys_path.as_ptr()), 1);

            tree_destroy(tree);
            scan_result_destroy(scan_result);
        }
    }
}
