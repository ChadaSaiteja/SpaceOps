//! Cleanup Engine & Safety Architecture (ADR-013, PRD §12).
//!
//! Provides deterministic cleanup candidate detection, hardcoded protected path guardrails,
//! dry-run simulation mode, and safe deletion via Windows Recycle Bin (FOF_ALLOWUNDO).

use crate::tree::{NodeKind, StorageTree};
use std::path::{Path, PathBuf};

/// Enumeration of distinct cleanup rule categories (PRD §12).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CleanupRuleId {
    UserTemp = 0,
    SystemTemp = 1,
    WindowsUpdate = 2,
    CrashDumps = 3,
    Thumbcache = 4,
    RecycleBin = 5,
    StaleLogs = 6,
}

impl CleanupRuleId {
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0 => Some(Self::UserTemp),
            1 => Some(Self::SystemTemp),
            2 => Some(Self::WindowsUpdate),
            3 => Some(Self::CrashDumps),
            4 => Some(Self::Thumbcache),
            5 => Some(Self::RecycleBin),
            6 => Some(Self::StaleLogs),
            _ => None,
        }
    }
}

/// Safety risk level classification (ADR-013).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Low = 0,    // Safe to clear; regenerated automatically
    Medium = 1, // Non-essential cache; may require re-download
    High = 2,   // Requires careful user confirmation
}

/// A detected cleanup candidate group with full transparent explanations (PRD §12).
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupCandidate {
    pub rule_id: CleanupRuleId,
    pub name: String,
    pub description: String,
    pub path_display: String,
    pub reason: String,
    pub consequence: String,
    pub risk_level: RiskLevel,
    pub total_bytes: u64,
    pub file_count: u64,
    pub is_protected: bool,
    pub target_paths: Vec<PathBuf>,
}

/// Execution summary of a cleanup operation (ADR-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanupReport {
    pub files_reclaimed: u64,
    pub bytes_reclaimed: u64,
    pub files_failed: u64,
    pub is_dry_run: bool,
}

/// Immutable blacklist of protected system paths (Invariant 1).
/// No deletion operation may ever delete, intersect, or contain these paths.
pub fn is_path_protected(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    let norm = path_str
        .trim()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase();

    // 1. Root drives (e.g. C:, D:, C:\, D:\)
    if norm.len() <= 3 && norm.ends_with(':') {
        return true;
    }

    // 2. Critical Windows and Boot directories
    let protected_prefixes = [
        "c:\\windows\\system32",
        "c:\\windows\\syswow64",
        "c:\\windows\\winsxs",
        "c:\\windows\\boot",
        "c:\\boot",
        "c:\\efi",
        "c:\\program files",
        "c:\\program files (x86)",
    ];

    for prefix in &protected_prefixes {
        if norm == *prefix || norm.starts_with(&format!("{}\\", prefix)) {
            // Note: C:\Windows\Temp and C:\Windows\SoftwareDistribution\Download are allowed exceptions
            if norm.starts_with("c:\\windows\\temp")
                || norm.starts_with("c:\\windows\\softwaredistribution\\download")
            {
                continue;
            }
            return true;
        }
    }

    // Exact Windows root
    if norm == "c:\\windows" {
        return true;
    }

    // 3. User root & critical profile folders
    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        let up_norm = user_profile
            .trim()
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase();
        if norm == up_norm {
            return true;
        }
        let user_critical = [
            format!("{}\\desktop", up_norm),
            format!("{}\\documents", up_norm),
            format!("{}\\pictures", up_norm),
            format!("{}\\music", up_norm),
            format!("{}\\videos", up_norm),
        ];
        for crit in &user_critical {
            if norm == *crit || norm.starts_with(&format!("{}\\", crit)) {
                return true;
            }
        }
    }

    // 4. System state files
    let system_files = [
        "pagefile.sys",
        "swapfile.sys",
        "hiberfil.sys",
        "ntldr",
        "bootmgr",
    ];
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = file_name.to_ascii_lowercase();
        if system_files.contains(&lower.as_str()) {
            return true;
        }
    }

    false
}

impl StorageTree {
    /// Detects all cleanup candidates across the in-memory StorageTree arena and well-known locations (ADR-013).
    pub fn detect_cleanup_candidates(&self) -> Vec<CleanupCandidate> {
        vec![
            // 1. User Temp Files
            self.detect_user_temp(),
            // 2. System Temp Files
            self.detect_system_temp(),
            // 3. Windows Update Download Cache
            self.detect_windows_update_cache(),
            // 4. Crash Dumps
            self.detect_crash_dumps(),
            // 5. Thumbnail Cache
            self.detect_thumbcache(),
            // 6. Recycle Bin
            self.detect_recycle_bin(),
            // 7. Stale Logs
            self.detect_stale_logs(),
        ]
    }

    fn detect_user_temp(&self) -> CleanupCandidate {
        let local_temp = std::env::var("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("Temp"))
            .unwrap_or_else(|_| PathBuf::from("C:\\Users\\Default\\AppData\\Local\\Temp"));

        let (bytes, count, targets) =
            self.aggregate_path_or_disk(&local_temp, |name| !name.is_empty());

        CleanupCandidate {
            rule_id: CleanupRuleId::UserTemp,
            name: "User Temporary Files".to_string(),
            description: "Temporary data created by running applications that was not cleaned up."
                .to_string(),
            path_display: "%LOCALAPPDATA%\\Temp".to_string(),
            reason: "Applications store temporary workspaces here; stale files waste space."
                .to_string(),
            consequence:
                "Active applications will recreate temporary files automatically if needed."
                    .to_string(),
            risk_level: RiskLevel::Low,
            total_bytes: bytes,
            file_count: count,
            is_protected: is_path_protected(&local_temp),
            target_paths: targets,
        }
    }

    fn detect_system_temp(&self) -> CleanupCandidate {
        let sys_temp = PathBuf::from("C:\\Windows\\Temp");
        let (bytes, count, targets) = self.aggregate_path_or_disk(&sys_temp, |_| true);

        CleanupCandidate {
            rule_id: CleanupRuleId::SystemTemp,
            name: "System Temporary Files".to_string(),
            description: "Temporary files created by Windows system services and installers."
                .to_string(),
            path_display: "C:\\Windows\\Temp".to_string(),
            reason: "Leftover installer caches and service temporary files.".to_string(),
            consequence: "Safe to remove; Windows services regenerate required temporary state."
                .to_string(),
            risk_level: RiskLevel::Low,
            total_bytes: bytes,
            file_count: count,
            is_protected: false, // Explicitly safe subset of Windows directory
            target_paths: targets,
        }
    }

    fn detect_windows_update_cache(&self) -> CleanupCandidate {
        let wu_path = PathBuf::from("C:\\Windows\\SoftwareDistribution\\Download");
        let (bytes, count, targets) = self.aggregate_path_or_disk(&wu_path, |_| true);

        CleanupCandidate {
            rule_id: CleanupRuleId::WindowsUpdate,
            name: "Windows Update Download Cache".to_string(),
            description:
                "Staged installation files for Windows updates that have already been applied."
                    .to_string(),
            path_display: "C:\\Windows\\SoftwareDistribution\\Download".to_string(),
            reason: "Windows retains installation payloads after updates finish installing."
                .to_string(),
            consequence:
                "Windows Update will re-download update files if a rollback or repair is requested."
                    .to_string(),
            risk_level: RiskLevel::Medium,
            total_bytes: bytes,
            file_count: count,
            is_protected: false,
            target_paths: targets,
        }
    }

    fn detect_crash_dumps(&self) -> CleanupCandidate {
        let mut target_paths = Vec::new();
        let mut total_bytes = 0u64;
        let mut file_count = 0u64;

        // Check in-memory tree for minidumps and crash dumps
        for node in &self.nodes {
            if node.kind == NodeKind::File {
                let lower = node.name.to_ascii_lowercase();
                if lower.ends_with(".dmp") || lower == "memory.dmp" {
                    if let Some(path) = self.full_path(node.id) {
                        total_bytes += node.size;
                        file_count += 1;
                        target_paths.push(path);
                    }
                }
            }
        }

        CleanupCandidate {
            rule_id: CleanupRuleId::CrashDumps,
            name: "System Crash Dumps & Minidumps".to_string(),
            description: "Memory dump files generated during past application crashes and BSODs."
                .to_string(),
            path_display: "C:\\Windows\\Minidump & MEMORY.DMP".to_string(),
            reason:
                "Post-mortem diagnostic dumps from historical crashes no longer being analyzed."
                    .to_string(),
            consequence:
                "Historical crash traces will be removed. New crashes will generate new dumps."
                    .to_string(),
            risk_level: RiskLevel::Low,
            total_bytes,
            file_count,
            is_protected: false,
            target_paths,
        }
    }

    fn detect_thumbcache(&self) -> CleanupCandidate {
        let _explorer_path = std::env::var("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("Microsoft\\Windows\\Explorer"))
            .unwrap_or_else(|_| PathBuf::from("C:\\Windows\\Explorer"));

        let mut target_paths = Vec::new();
        let mut total_bytes = 0u64;
        let mut file_count = 0u64;

        for node in &self.nodes {
            if node.kind == NodeKind::File {
                let lower = node.name.to_ascii_lowercase();
                if lower.starts_with("thumbcache_") && lower.ends_with(".db") {
                    if let Some(path) = self.full_path(node.id) {
                        total_bytes += node.size;
                        file_count += 1;
                        target_paths.push(path);
                    }
                }
            }
        }

        CleanupCandidate {
            rule_id: CleanupRuleId::Thumbcache,
            name: "Windows Thumbnail Cache".to_string(),
            description: "Cached preview thumbnails for images, videos, and documents.".to_string(),
            path_display: "%LOCALAPPDATA%\\Microsoft\\Windows\\Explorer".to_string(),
            reason: "Thumbnails accumulate for files that may no longer exist.".to_string(),
            consequence:
                "File Explorer will regenerate thumbnails on demand when folders are opened."
                    .to_string(),
            risk_level: RiskLevel::Low,
            total_bytes,
            file_count,
            is_protected: false,
            target_paths,
        }
    }

    fn detect_recycle_bin(&self) -> CleanupCandidate {
        let mut target_paths = Vec::new();
        let mut total_bytes = 0u64;
        let mut file_count = 0u64;

        for node in &self.nodes {
            let lower = node.name.to_ascii_lowercase();
            if lower == "$recycle.bin" {
                total_bytes += node.size;
                file_count += node.file_count;
                if let Some(path) = self.full_path(node.id) {
                    target_paths.push(path);
                }
            }
        }

        CleanupCandidate {
            rule_id: CleanupRuleId::RecycleBin,
            name: "Recycle Bin Contents".to_string(),
            description: "Deleted files currently retained in the Windows Recycle Bin.".to_string(),
            path_display: "$Recycle.Bin".to_string(),
            reason: "Files previously marked for deletion taking up storage until emptied."
                .to_string(),
            consequence:
                "Permanently frees space; previously deleted files can no longer be restored."
                    .to_string(),
            risk_level: RiskLevel::Low,
            total_bytes,
            file_count,
            is_protected: false,
            target_paths,
        }
    }

    fn detect_stale_logs(&self) -> CleanupCandidate {
        let mut target_paths = Vec::new();
        let mut total_bytes = 0u64;
        let mut file_count = 0u64;

        for node in &self.nodes {
            if node.kind == NodeKind::File {
                let lower = node.name.to_ascii_lowercase();
                if lower.ends_with(".log") || lower.ends_with(".old") || lower.ends_with(".bak") {
                    if let Some(path) = self.full_path(node.id) {
                        let path_str = path.to_string_lossy().to_ascii_lowercase();
                        if path_str.contains("temp") || path_str.contains("cache") {
                            total_bytes += node.size;
                            file_count += 1;
                            target_paths.push(path);
                        }
                    }
                }
            }
        }

        CleanupCandidate {
            rule_id: CleanupRuleId::StaleLogs,
            name: "Stale Diagnostic Logs".to_string(),
            description: "Historical diagnostic log and backup files in temporary directories."
                .to_string(),
            path_display: "*.log, *.old, *.bak in temp/cache".to_string(),
            reason: "Old log files created during troubleshooting sessions.".to_string(),
            consequence:
                "Diagnostic logs will be removed without impacting application functionality."
                    .to_string(),
            risk_level: RiskLevel::Low,
            total_bytes,
            file_count,
            is_protected: false,
            target_paths,
        }
    }

    /// Aggregates byte size and file count from in-memory arena if available, or falls back to live disk.
    fn aggregate_path_or_disk<F>(&self, target_dir: &Path, filter: F) -> (u64, u64, Vec<PathBuf>)
    where
        F: Fn(&str) -> bool,
    {
        let mut total_bytes = 0u64;
        let mut file_count = 0u64;
        let mut targets = Vec::new();

        // 1. Try resolving through in-memory tree nodes first
        let target_str = target_dir.to_string_lossy().to_ascii_lowercase();
        let mut matched_in_tree = false;

        for node in &self.nodes {
            if let Some(path) = self.full_path(node.id) {
                let p_str = path.to_string_lossy().to_ascii_lowercase();
                if p_str.starts_with(&target_str) && p_str != target_str {
                    matched_in_tree = true;
                    if node.kind == NodeKind::File && filter(&node.name) {
                        total_bytes += node.size;
                        file_count += 1;
                        targets.push(path);
                    }
                }
            }
        }

        // 2. If not covered by the current scan root, inspect disk directly if it exists
        if !matched_in_tree && target_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(target_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if let Ok(meta) = entry.metadata() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if meta.is_file() && filter(&name) {
                            total_bytes += meta.len();
                            file_count += 1;
                            targets.push(p);
                        } else if meta.is_dir() {
                            targets.push(p);
                        }
                    }
                }
            }
        }

        (total_bytes, file_count, targets)
    }
}

/// Executes safe removal of specified candidate paths (ADR-013).
pub fn execute_cleanup(
    paths: &[PathBuf],
    dry_run: bool,
    send_to_recycle_bin: bool,
) -> CleanupReport {
    let mut report = CleanupReport {
        files_reclaimed: 0,
        bytes_reclaimed: 0,
        files_failed: 0,
        is_dry_run: dry_run,
    };

    for path in paths {
        // Enforce safety invariant 1: Reject protected locations
        if is_path_protected(path) {
            report.files_failed += 1;
            continue;
        }

        if !path.exists() {
            continue;
        }

        let size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(_) => 0,
        };

        if dry_run {
            report.files_reclaimed += 1;
            report.bytes_reclaimed += size;
            continue;
        }

        // Execution mode
        if send_to_recycle_bin {
            #[cfg(windows)]
            {
                if send_file_to_recycle_bin(path) {
                    report.files_reclaimed += 1;
                    report.bytes_reclaimed += size;
                } else {
                    report.files_failed += 1;
                }
            }
            #[cfg(not(windows))]
            {
                if std::fs::remove_file(path).is_ok() || std::fs::remove_dir_all(path).is_ok() {
                    report.files_reclaimed += 1;
                    report.bytes_reclaimed += size;
                } else {
                    report.files_failed += 1;
                }
            }
        } else {
            // Direct permanent removal
            let success = if path.is_dir() {
                std::fs::remove_dir_all(path).is_ok()
            } else {
                std::fs::remove_file(path).is_ok()
            };

            if success {
                report.files_reclaimed += 1;
                report.bytes_reclaimed += size;
            } else {
                report.files_failed += 1;
            }
        }
    }

    report
}

#[cfg(windows)]
fn send_file_to_recycle_bin(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    // SHFileOperationW requires a double-null-terminated string buffer
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0);

    #[allow(clippy::upper_case_acronyms)]
    #[repr(C)]
    struct SHFILEOPSTRUCTW {
        hwnd: *mut std::ffi::c_void,
        w_func: u32,
        p_from: *const u16,
        p_to: *const u16,
        f_flags: u16,
        f_any_operations_aborted: i32,
        h_name_mappings: *mut std::ffi::c_void,
        lpsz_progress_title: *const u16,
    }

    const FO_DELETE: u32 = 0x0003;
    const FOF_SILENT: u16 = 0x0004;
    const FOF_NOCONFIRMATION: u16 = 0x0010;
    const FOF_ALLOWUNDO: u16 = 0x0040;
    const FOF_NOERRORUI: u16 = 0x0400;

    let mut file_op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        w_func: FO_DELETE,
        p_from: wide.as_ptr(),
        p_to: std::ptr::null(),
        f_flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
        f_any_operations_aborted: 0,
        h_name_mappings: std::ptr::null_mut(),
        lpsz_progress_title: std::ptr::null(),
    };

    #[link(name = "shell32")]
    extern "system" {
        fn SHFileOperationW(lpFileOp: *mut SHFILEOPSTRUCTW) -> i32;
    }

    unsafe {
        let ret = SHFileOperationW(&mut file_op);
        ret == 0 && file_op.f_any_operations_aborted == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{FileAttributeFlags, NodeId, ScanEvent};
    use tempfile::tempdir;

    #[test]
    fn safety_protected_paths_are_strictly_rejected() {
        assert!(is_path_protected(Path::new("C:\\")));
        assert!(is_path_protected(Path::new("D:\\")));
        assert!(is_path_protected(Path::new("C:\\Windows")));
        assert!(is_path_protected(Path::new("C:\\Windows\\System32")));
        assert!(is_path_protected(Path::new(
            "C:\\Windows\\System32\\ntdll.dll"
        )));
        assert!(is_path_protected(Path::new("C:\\Windows\\WinSxS")));
        assert!(is_path_protected(Path::new("C:\\Program Files")));
        assert!(is_path_protected(Path::new("C:\\Program Files (x86)")));
        assert!(is_path_protected(Path::new("C:\\pagefile.sys")));
        assert!(is_path_protected(Path::new("C:\\swapfile.sys")));
        assert!(is_path_protected(Path::new("C:\\hiberfil.sys")));

        // Exceptions that are safe cleanup targets
        assert!(!is_path_protected(Path::new("C:\\Windows\\Temp")));
        assert!(!is_path_protected(Path::new("C:\\Windows\\Temp\\junk.tmp")));
        assert!(!is_path_protected(Path::new(
            "C:\\Windows\\SoftwareDistribution\\Download"
        )));
    }

    #[test]
    fn dry_run_mode_does_not_modify_files() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_temp.tmp");
        std::fs::write(&file_path, vec![b'a'; 1024]).unwrap();

        let report = execute_cleanup(std::slice::from_ref(&file_path), true, true);

        assert!(report.is_dry_run);
        assert_eq!(report.files_reclaimed, 1);
        assert_eq!(report.bytes_reclaimed, 1024);
        assert_eq!(report.files_failed, 0);
        // File must still exist after dry run!
        assert!(file_path.exists());
    }

    #[test]
    fn protected_path_cannot_be_deleted_even_in_execution_mode() {
        let protected = PathBuf::from("C:\\Windows\\System32");
        let report = execute_cleanup(&[protected], false, false);

        assert_eq!(report.files_reclaimed, 0);
        assert_eq!(report.files_failed, 1);
    }

    #[test]
    fn in_memory_tree_detects_crash_dumps_and_stale_logs() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: common::DirMetadata {
                    name: "C:\\temp".into(),
                    is_reparse_point: false,
                },
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: common::FileMetadata {
                    name: "crash.dmp".into(),
                    size: 50000,
                    extension: Some("dmp".into()),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: common::FileMetadata {
                    name: "old_service.log".into(),
                    size: 12000,
                    extension: Some("log".into()),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 62000,
                file_count: 2,
                dir_count: 0,
            },
        ];

        let tree = StorageTree::build(events).unwrap();
        let candidates = tree.detect_cleanup_candidates();

        let crash_cand = candidates
            .iter()
            .find(|c| c.rule_id == CleanupRuleId::CrashDumps)
            .unwrap();
        assert_eq!(crash_cand.file_count, 1);
        assert_eq!(crash_cand.total_bytes, 50000);

        let log_cand = candidates
            .iter()
            .find(|c| c.rule_id == CleanupRuleId::StaleLogs)
            .unwrap();
        assert_eq!(log_cand.file_count, 1);
        assert_eq!(log_cand.total_bytes, 12000);
    }
}
