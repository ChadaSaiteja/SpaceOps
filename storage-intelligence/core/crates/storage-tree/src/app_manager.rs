//! Application Management & Storage Attribution Engine (ADR-014, PRD §14).
//!
//! Provides installed application discovery across Win32 registry roots and MSIX packages,
//! high-fidelity storage attribution via the in-memory `StorageTree` arena, and orphaned
//! leftover detection across user and system application cache roots.

use crate::cleanup::RiskLevel;
use crate::tree::StorageTree;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use windows::core::{PCWSTR, PWSTR};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_DWORD, REG_EXPAND_SZ,
    REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
};

/// Application deployment format / packaging type.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    Win32 = 0,
    Msix = 1,
}

/// Discovered installed application with storage attribution (ADR-014).
#[derive(Debug, Clone, PartialEq)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub install_location: String,
    pub uninstall_string: String,
    pub quiet_uninstall_string: String,
    pub install_date: String,
    pub kind: AppKind,
    pub estimated_size: u64,
    pub actual_size: u64,
    pub file_count: u64,
    pub is_system_component: bool,
}

/// Categorized location for application residue / leftover.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftoverLocationType {
    LocalAppData = 0,
    AppDataRoaming = 1,
    ProgramData = 2,
}

/// An identified orphaned leftover folder from previous installations (ADR-014 §4).
#[derive(Debug, Clone, PartialEq)]
pub struct AppLeftover {
    pub id: String,
    pub app_name: String,
    pub path: PathBuf,
    pub size: u64,
    pub file_count: u64,
    pub location_type: LeftoverLocationType,
    pub risk_level: RiskLevel,
}

#[cfg(windows)]
fn read_string_value(key: HKEY, value_name: &str) -> Option<String> {
    let wide_name: Vec<u16> = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut val_type = REG_VALUE_TYPE(0);
    let mut data_len: u32 = 0;

    unsafe {
        if RegQueryValueExW(
            key,
            PCWSTR(wide_name.as_ptr()),
            None,
            Some(&mut val_type),
            None,
            Some(&mut data_len),
        )
        .is_err()
        {
            return None;
        }
        if val_type != REG_SZ && val_type != REG_EXPAND_SZ {
            return None;
        }
        if data_len == 0 {
            return Some(String::new());
        }
        let u16_len = (data_len as usize) / 2;
        let mut buf: Vec<u16> = vec![0; u16_len];
        if RegQueryValueExW(
            key,
            PCWSTR(wide_name.as_ptr()),
            None,
            Some(&mut val_type),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut data_len),
        )
        .is_err()
        {
            return None;
        }
        while let Some(&0) = buf.last() {
            buf.pop();
        }
        Some(String::from_utf16_lossy(&buf))
    }
}

#[cfg(windows)]
fn read_dword_value(key: HKEY, value_name: &str) -> Option<u32> {
    let wide_name: Vec<u16> = value_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut val_type = REG_VALUE_TYPE(0);
    let mut dword: u32 = 0;
    let mut data_len: u32 = std::mem::size_of::<u32>() as u32;

    unsafe {
        if RegQueryValueExW(
            key,
            PCWSTR(wide_name.as_ptr()),
            None,
            Some(&mut val_type),
            Some(&mut dword as *mut u32 as *mut u8),
            Some(&mut data_len),
        )
        .is_ok()
            && val_type == REG_DWORD
        {
            return Some(dword);
        }
    }
    None
}

#[cfg(windows)]
fn enumerate_subkeys(root: HKEY, subkey_path: &str, sam: REG_SAM_FLAGS) -> Vec<String> {
    let wide_path: Vec<u16> = subkey_path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut key = HKEY::default();
    let mut result = Vec::new();

    unsafe {
        if RegOpenKeyExW(root, PCWSTR(wide_path.as_ptr()), 0, sam, &mut key).is_err() {
            return result;
        }
        let mut index = 0u32;
        let mut name_buf = [0u16; 256];
        loop {
            let mut name_len = name_buf.len() as u32;
            let status = RegEnumKeyExW(
                key,
                index,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                PWSTR::null(),
                None,
                None,
            );
            if status.is_err() {
                break;
            }
            let subkey_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            result.push(subkey_name);
            index += 1;
        }
        let _ = RegCloseKey(key);
    }
    result
}

/// Scans standard Windows Registry uninstall locations for installed Win32 software.
pub fn discover_win32_apps(tree: Option<&StorageTree>) -> Vec<AppInfo> {
    let mut apps = Vec::new();
    let mut seen_ids = HashSet::new();

    #[cfg(windows)]
    {
        const UNINSTALL_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

        // Scans HKLM 64-bit, HKLM 32-bit (WoW64), and HKCU (current user)
        let targets: &[(HKEY, REG_SAM_FLAGS)] = &[
            (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY),
            (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY),
            (HKEY_CURRENT_USER, KEY_READ),
        ];

        for &(root, sam) in targets {
            let subkeys = enumerate_subkeys(root, UNINSTALL_PATH, sam);
            for subkey in subkeys {
                if seen_ids.contains(&subkey) {
                    continue;
                }

                let full_subpath = format!(r"{}\{}", UNINSTALL_PATH, subkey);
                let wide_subpath: Vec<u16> = full_subpath
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let mut key = HKEY::default();

                unsafe {
                    if RegOpenKeyExW(root, PCWSTR(wide_subpath.as_ptr()), 0, sam, &mut key).is_err()
                    {
                        continue;
                    }

                    let display_name = read_string_value(key, "DisplayName");
                    if display_name.as_deref().unwrap_or("").trim().is_empty() {
                        let _ = RegCloseKey(key);
                        continue;
                    }

                    // Check for system update filters
                    let parent_key = read_string_value(key, "ParentKeyName");
                    let release_type = read_string_value(key, "ReleaseType");
                    let sys_comp = read_dword_value(key, "SystemComponent").unwrap_or(0);

                    // Skip minor hotfixes/updates
                    if parent_key.is_some()
                        || release_type.as_deref() == Some("Update")
                        || release_type.as_deref() == Some("Hotfix")
                    {
                        let _ = RegCloseKey(key);
                        continue;
                    }

                    let name = display_name.unwrap().trim().to_string();
                    let version = read_string_value(key, "DisplayVersion").unwrap_or_default();
                    let publisher = read_string_value(key, "Publisher").unwrap_or_default();
                    let install_location =
                        read_string_value(key, "InstallLocation").unwrap_or_default();
                    let uninstall_string =
                        read_string_value(key, "UninstallString").unwrap_or_default();
                    let quiet_uninstall_string =
                        read_string_value(key, "QuietUninstallString").unwrap_or_default();
                    let install_date = read_string_value(key, "InstallDate").unwrap_or_default();
                    let estimated_size_kb =
                        read_dword_value(key, "EstimatedSize").unwrap_or(0) as u64;

                    let _ = RegCloseKey(key);

                    seen_ids.insert(subkey.clone());

                    let mut app = AppInfo {
                        id: subkey,
                        name,
                        version,
                        publisher,
                        install_location,
                        uninstall_string,
                        quiet_uninstall_string,
                        install_date,
                        kind: AppKind::Win32,
                        estimated_size: estimated_size_kb * 1024,
                        actual_size: 0,
                        file_count: 0,
                        is_system_component: sys_comp == 1,
                    };

                    attribute_app_size(&mut app, tree);
                    apps.push(app);
                }
            }
        }
    }

    // Sort by actual size descending, then by name
    apps.sort_by(|a, b| {
        b.actual_size
            .cmp(&a.actual_size)
            .then_with(|| a.name.cmp(&b.name))
    });

    apps
}

/// Attributes true storage footprint to an `AppInfo` using the active `StorageTree`.
/// If the application's install location exists in the tree, exact cluster-allocated
/// bytes and file counts are populated in O(depth).
pub fn attribute_app_size(app: &mut AppInfo, tree: Option<&StorageTree>) {
    let mut resolved_from_tree = false;

    if let Some(t) = tree {
        if !app.install_location.is_empty() {
            let path = Path::new(&app.install_location);
            if let Some(node_id) = t.find_by_path(path) {
                if let Some(node) = t.node(node_id) {
                    app.actual_size = node.size;
                    app.file_count = node.file_count;
                    resolved_from_tree = true;
                }
            }
        }

        // If install_location was blank or not found, attempt to infer from uninstall_string
        if !resolved_from_tree && !app.uninstall_string.is_empty() {
            if let Some(inferred_dir) = infer_directory_from_command(&app.uninstall_string) {
                if let Some(node_id) = t.find_by_path(&inferred_dir) {
                    if let Some(node) = t.node(node_id) {
                        app.actual_size = node.size;
                        app.file_count = node.file_count;
                        if app.install_location.is_empty() {
                            app.install_location = inferred_dir.to_string_lossy().into_owned();
                        }
                        resolved_from_tree = true;
                    }
                }
            }
        }
    }

    // Fallback: If not found in tree, use estimated_size from registry if non-zero
    if !resolved_from_tree && app.actual_size == 0 && app.estimated_size > 0 {
        app.actual_size = app.estimated_size;
    }
}

/// Helper to extract directory from a command line string (e.g. "\"C:\Program Files\App\unins000.exe\"").
pub fn infer_directory_from_command(cmd: &str) -> Option<PathBuf> {
    let trimmed = cmd.trim();
    let unquoted = if trimmed.starts_with('"') {
        trimmed.strip_prefix('"')?.split('"').next()?
    } else {
        trimmed.split_whitespace().next()?
    };

    let p = Path::new(unquoted);
    if p.is_file() || p.extension().is_some() {
        p.parent().map(|p| p.to_path_buf())
    } else if p.is_dir() {
        Some(p.to_path_buf())
    } else {
        None
    }
}

/// Detects orphaned leftover directories across AppData and ProgramData (ADR-014 §4).
/// Cross-references existing directories with currently installed applications.
pub fn find_app_leftovers(
    installed_apps: &[AppInfo],
    tree: Option<&StorageTree>,
) -> Vec<AppLeftover> {
    let mut leftovers = Vec::new();

    // Standard system exclusions that must never be flagged as leftovers
    let system_ignore: HashSet<&str> = [
        "microsoft",
        "windows",
        "packages",
        "temp",
        "system",
        "common files",
        "nvidia",
        "intel",
        "amd",
        "realtek",
        "adobe",
        "google",
        "mozilla",
        "microsoft corporation",
        "windows defender",
        "windows defender advanced threat protection",
        "dotnet",
        "git",
        "powershell",
        "nuget",
        "pip",
        "yarn",
        "npm",
        "cargo",
    ]
    .iter()
    .cloned()
    .collect();

    // Create a lookup of lowercased installed application and publisher tokens
    let mut app_tokens: HashSet<String> = HashSet::new();
    for app in installed_apps {
        for word in app.name.split_whitespace() {
            let clean = word
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase();
            if clean.len() >= 3 {
                app_tokens.insert(clean);
            }
        }
        for word in app.publisher.split_whitespace() {
            let clean = word
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase();
            if clean.len() >= 3 {
                app_tokens.insert(clean);
            }
        }
    }

    // Inspect user and system roots
    let candidate_roots: &[(LeftoverLocationType, Option<PathBuf>)] = &[
        (
            LeftoverLocationType::LocalAppData,
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        ),
        (
            LeftoverLocationType::AppDataRoaming,
            std::env::var_os("APPDATA").map(PathBuf::from),
        ),
        (
            LeftoverLocationType::ProgramData,
            std::env::var_os("ProgramData")
                .map(PathBuf::from)
                .or_else(|| Some(PathBuf::from(r"C:\ProgramData"))),
        ),
    ];

    for &(loc_type, ref root_opt) in candidate_roots {
        let root = match root_opt {
            Some(r) if r.is_dir() => r,
            _ => continue,
        };

        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let folder_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            let folder_lower = folder_name.to_ascii_lowercase();
            if system_ignore.contains(folder_lower.as_str()) {
                continue;
            }

            // Check if folder name matches any currently installed app token
            let has_matching_app = app_tokens.contains(&folder_lower)
                || app_tokens.iter().any(|t| folder_lower.contains(t.as_str()));

            if !has_matching_app {
                // Potential leftover directory!
                let (size, file_count) = if let Some(t) = tree {
                    if let Some(node_id) = t.find_by_path(&path) {
                        if let Some(node) = t.node(node_id) {
                            (node.size, node.file_count)
                        } else {
                            (0, 0)
                        }
                    } else {
                        compute_quick_dir_size(&path)
                    }
                } else {
                    compute_quick_dir_size(&path)
                };

                // Only report if there is non-trivial content (> 100 KB or >= 5 files)
                if size > 102_400 || file_count >= 5 {
                    leftovers.push(AppLeftover {
                        id: format!("{:?}_{}", loc_type, folder_name),
                        app_name: folder_name.to_string(),
                        path,
                        size,
                        file_count,
                        location_type: loc_type,
                        risk_level: RiskLevel::Low,
                    });
                }
            }
        }
    }

    leftovers.sort_by_key(|a| std::cmp::Reverse(a.size));
    leftovers
}

/// Fallback bounded on-disk size calculator when directory is not inside active StorageTree.
fn compute_quick_dir_size(dir: &Path) -> (u64, u64) {
    compute_quick_dir_size_bounded(dir, 0, 3)
}

fn compute_quick_dir_size_bounded(dir: &Path, depth: usize, max_depth: usize) -> (u64, u64) {
    if depth > max_depth {
        return (0, 0);
    }
    let mut total_size = 0u64;
    let mut file_count = 0u64;

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten().take(500) {
            let p = entry.path();
            if p.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total_size += meta.len();
                    file_count += 1;
                }
            } else if p.is_dir() {
                let (sub_size, sub_files) =
                    compute_quick_dir_size_bounded(&p, depth + 1, max_depth);
                total_size += sub_size;
                file_count += sub_files;
            }
        }
    }

    (total_size, file_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{NodeId, ScanEvent};

    fn dir_meta(name: &str) -> common::DirMetadata {
        common::DirMetadata {
            name: std::ffi::OsString::from(name),
            is_reparse_point: false,
        }
    }

    fn file_meta(name: &str, size: u64) -> common::FileMetadata {
        common::FileMetadata {
            name: std::ffi::OsString::from(name),
            size,
            extension: Some("exe".to_string()),
            modified: None,
            created: None,
            attributes: common::FileAttributeFlags::default(),
        }
    }

    #[test]
    fn attribute_app_size_from_tree() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta(r"C:\Program Files"),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: dir_meta("SuperEditor"),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(1),
                meta: file_meta("editor.exe", 52428800), // 50 MB
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(1),
                total_size: 52428800,
                file_count: 1,
                dir_count: 0,
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 52428800,
                file_count: 1,
                dir_count: 1,
            },
        ];

        let tree = StorageTree::build(events).unwrap();

        let mut app = AppInfo {
            id: "SuperEditor_1".to_string(),
            name: "Super Editor".to_string(),
            version: "2.0".to_string(),
            publisher: "CodeCorp".to_string(),
            install_location: r"C:\Program Files\SuperEditor".to_string(),
            uninstall_string: r"C:\Program Files\SuperEditor\unins000.exe".to_string(),
            quiet_uninstall_string: String::new(),
            install_date: "20260101".to_string(),
            kind: AppKind::Win32,
            estimated_size: 1024,
            actual_size: 0,
            file_count: 0,
            is_system_component: false,
        };

        attribute_app_size(&mut app, Some(&tree));

        assert_eq!(app.actual_size, 52428800);
        assert_eq!(app.file_count, 1);
    }

    #[test]
    fn infer_directory_from_uninstall_command() {
        let cmd = r#""C:\Program Files (x86)\Vendor\Tool\uninstall.exe" /S"#;
        let inferred = infer_directory_from_command(cmd);
        assert_eq!(
            inferred,
            Some(PathBuf::from(r"C:\Program Files (x86)\Vendor\Tool"))
        );

        let unquoted = r#"C:\Apps\Simple\uninst.exe"#;
        assert_eq!(
            infer_directory_from_command(unquoted),
            Some(PathBuf::from(r"C:\Apps\Simple"))
        );
    }

    #[test]
    fn compute_quick_dir_size_on_temp_dir() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"12345").unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("b.txt"), b"1234567890").unwrap();

        let (size, files) = compute_quick_dir_size(dir.path());
        assert_eq!(size, 15);
        assert_eq!(files, 2);
    }
}
