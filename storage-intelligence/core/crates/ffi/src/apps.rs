//! Application manager and leftover detection exports (ADR-014).

use crate::error::{catch_ffi_panic, ScanErrorCode};
use crate::handles::TreeHandle;
use crate::marshal::copy_str_to_wide_buf;

/// Opaque handle wrapping discovered applications (ADR-014).
pub struct AppCatalogHandle(pub(crate) Vec<storage_tree::AppInfo>);

/// Opaque handle wrapping discovered leftovers (ADR-014).
pub struct AppLeftoversHandle(pub(crate) Vec<storage_tree::AppLeftover>);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

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
}
