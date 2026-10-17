//! Developer artifact catalog and safe reclamation exports (ADR-015).

use crate::cleanup::CleanupReportFfi;
use crate::error::{catch_ffi_panic, ScanErrorCode};
use crate::handles::TreeHandle;
use crate::marshal::{copy_str_to_wide_buf, wide_ptr_to_pathbuf};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

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
}
