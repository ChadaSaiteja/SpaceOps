//! Developer Storage Intelligence & Safe Reclamation Engine (ADR-015, PRD §13).
//!
//! Provides deterministic discovery of project dependencies, build artifacts, global package
//! caches, and container disks across active StorageTree arenas and canonical Windows dev paths.

use crate::cleanup::{execute_cleanup, is_path_protected, CleanupReport, RiskLevel};
use crate::tree::{NodeKind, StorageTree};
use common::NodeId;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Programming ecosystem / toolchain classification (ADR-015 §1).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevEcosystem {
    NodeJs = 0,
    Rust = 1,
    DotNet = 2,
    Python = 3,
    Java = 4,
    DockerWsl = 5,
    Git = 6,
    Other = 7,
}

/// Category of developer storage artifact.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevArtifactKind {
    ProjectArtifact = 0, // node_modules, target/, bin/obj, .venv
    GlobalCache = 1,     // NuGet packages, Cargo cache, npm cache, pip cache
    VirtualDisk = 2,     // Docker ext4.vhdx, WSL vhdx
}

/// Discovered developer artifact with storage, dormancy, and safety metadata (ADR-015).
#[derive(Debug, Clone, PartialEq)]
pub struct DevArtifact {
    pub id: String,
    pub ecosystem: DevEcosystem,
    pub kind: DevArtifactKind,
    pub name: String,
    pub project_name: String,
    pub path: PathBuf,
    pub size: u64,
    pub file_count: u64,
    pub is_dormant: bool,
    pub days_inactive: u32,
    pub risk_level: RiskLevel,
    pub description: String,
    pub cleanup_command: String,
}

/// Detects all developer artifacts across the in-memory StorageTree and canonical global paths.
pub fn detect_dev_artifacts(tree: Option<&StorageTree>) -> Vec<DevArtifact> {
    let mut artifacts = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    // 1. Scan in-memory tree arena (sub-10ms)
    if let Some(t) = tree {
        let node_count = t.node_count();
        let now = SystemTime::now();

        for i in 0..node_count {
            let id = NodeId(i as u64);
            if let Some(node) = t.node(id) {
                if node.kind != NodeKind::Directory {
                    continue;
                }

                let name_lower = node.name.to_ascii_lowercase();
                let matched_meta = match name_lower.as_str() {
                    "node_modules" => Some((
                        DevEcosystem::NodeJs,
                        DevArtifactKind::ProjectArtifact,
                        RiskLevel::Low,
                        "Node.js dependencies; recreated via 'npm install'",
                        "npm install / pnpm install",
                    )),
                    "target" => Some((
                        DevEcosystem::Rust,
                        DevArtifactKind::ProjectArtifact,
                        RiskLevel::Low,
                        "Cargo build artifacts; recreated via 'cargo build'",
                        "cargo clean",
                    )),
                    "bin" | "obj" => Some((
                        DevEcosystem::DotNet,
                        DevArtifactKind::ProjectArtifact,
                        RiskLevel::Low,
                        ".NET compiled binaries; recreated via 'dotnet build'",
                        "dotnet clean",
                    )),
                    ".venv" | "venv" => Some((
                        DevEcosystem::Python,
                        DevArtifactKind::ProjectArtifact,
                        RiskLevel::Low,
                        "Python virtual environment; recreated via 'python -m venv'",
                        "python -m venv .venv",
                    )),
                    "__pycache__" => Some((
                        DevEcosystem::Python,
                        DevArtifactKind::ProjectArtifact,
                        RiskLevel::Low,
                        "Python bytecode cache; recreated automatically by interpreter",
                        "Automatic",
                    )),
                    ".gradle" => Some((
                        DevEcosystem::Java,
                        DevArtifactKind::ProjectArtifact,
                        RiskLevel::Low,
                        "Gradle build cache; recreated via 'gradle build'",
                        "gradle clean",
                    )),
                    _ => None,
                };

                if let Some((ecosystem, kind, risk, desc, cmd)) = matched_meta {
                    if node.size > 0 {
                        if let Some(path) = t.full_path(id) {
                            let path_key = path.to_string_lossy().to_ascii_lowercase();
                            if !seen_paths.insert(path_key) {
                                continue;
                            }

                            // Extract project name from parent node
                            let project_name = node
                                .parent
                                .and_then(|pid| t.node(pid))
                                .map(|pnode| {
                                    Path::new(&pnode.name)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(pnode.name)
                                        .to_string()
                                })
                                .unwrap_or_else(|| "Unknown Project".to_string());

                            // Calculate dormancy from modification time
                            let (is_dormant, days_inactive) = match node.modified {
                                Some(mod_time) => {
                                    let days = now
                                        .duration_since(mod_time)
                                        .map(|d| (d.as_secs() / 86400) as u32)
                                        .unwrap_or(0);
                                    (days >= 30, days)
                                }
                                None => (false, 0),
                            };

                            artifacts.push(DevArtifact {
                                id: format!("{:?}_{}", ecosystem, path.display()),
                                ecosystem,
                                kind,
                                name: node.name.to_string(),
                                project_name,
                                path,
                                size: node.size,
                                file_count: node.file_count,
                                is_dormant,
                                days_inactive,
                                risk_level: risk,
                                description: desc.to_string(),
                                cleanup_command: cmd.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 2. Scan canonical global developer caches on Windows
    probe_global_dev_caches(&mut artifacts, &mut seen_paths, tree);

    // Sort by size descending
    artifacts.sort_by_key(|a| std::cmp::Reverse(a.size));
    artifacts
}

/// Probes canonical global package caches, stores, and container disks on Windows.
fn probe_global_dev_caches(
    artifacts: &mut Vec<DevArtifact>,
    seen_paths: &mut std::collections::HashSet<String>,
    tree: Option<&StorageTree>,
) {
    let user_profile = std::env::var_os("USERPROFILE").map(PathBuf::from);
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);

    #[allow(clippy::type_complexity)]
    let candidates: &[(
        DevEcosystem,
        DevArtifactKind,
        RiskLevel,
        &str,
        &str,
        &str,
        Option<PathBuf>,
    )] = &[
        // NuGet packages
        (
            DevEcosystem::DotNet,
            DevArtifactKind::GlobalCache,
            RiskLevel::Medium,
            "Global NuGet Package Cache",
            "Cached NuGet package tarballs; re-downloaded on build",
            "dotnet nuget locals all --clear",
            user_profile
                .as_ref()
                .map(|p| p.join(".nuget").join("packages")),
        ),
        // Cargo registry cache
        (
            DevEcosystem::Rust,
            DevArtifactKind::GlobalCache,
            RiskLevel::Medium,
            "Cargo Registry Package Cache",
            "Crates downloaded by cargo; re-downloaded when building",
            "cargo cache -a",
            user_profile
                .as_ref()
                .map(|p| p.join(".cargo").join("registry").join("cache")),
        ),
        // Cargo git checkouts
        (
            DevEcosystem::Rust,
            DevArtifactKind::GlobalCache,
            RiskLevel::Medium,
            "Cargo Git Dependencies Cache",
            "Git repository clones checked out by cargo",
            "cargo cache -g",
            user_profile
                .as_ref()
                .map(|p| p.join(".cargo").join("git").join("db")),
        ),
        // npm cache
        (
            DevEcosystem::NodeJs,
            DevArtifactKind::GlobalCache,
            RiskLevel::Medium,
            "Global npm Cache",
            "Downloaded npm package tarballs; re-downloaded as needed",
            "npm cache clean --force",
            local_app_data.as_ref().map(|p| p.join("npm-cache")),
        ),
        // pnpm store
        (
            DevEcosystem::NodeJs,
            DevArtifactKind::GlobalCache,
            RiskLevel::Medium,
            "Global pnpm Content Store",
            "Shared content-addressable pnpm package files",
            "pnpm store prune",
            local_app_data
                .as_ref()
                .map(|p| p.join("pnpm").join("store")),
        ),
        // pip cache
        (
            DevEcosystem::Python,
            DevArtifactKind::GlobalCache,
            RiskLevel::Medium,
            "Global pip Wheel Cache",
            "Cached Python wheels and tarballs; re-downloaded on install",
            "pip cache purge",
            local_app_data.as_ref().map(|p| p.join("pip").join("cache")),
        ),
        // Gradle caches
        (
            DevEcosystem::Java,
            DevArtifactKind::GlobalCache,
            RiskLevel::Medium,
            "Gradle Global Cache",
            "Downloaded Java artifacts, wrappers, and caches",
            "gradle --stop && rm .gradle/caches",
            user_profile
                .as_ref()
                .map(|p| p.join(".gradle").join("caches")),
        ),
        // Docker WSL virtual disk
        (
            DevEcosystem::DockerWsl,
            DevArtifactKind::VirtualDisk,
            RiskLevel::High,
            "Docker Desktop WSL Virtual Disk",
            "Virtual ext4.vhdx storing all Docker container layers and volumes",
            "docker system prune -a --volumes",
            local_app_data
                .as_ref()
                .map(|p| p.join("Docker").join("wsl").join("data").join("ext4.vhdx")),
        ),
    ];

    for &(ecosystem, kind, risk, name, desc, cmd, ref path_opt) in candidates {
        let path = match path_opt {
            Some(p) if p.exists() => p,
            _ => continue,
        };

        let path_key = path.to_string_lossy().to_ascii_lowercase();
        if !seen_paths.insert(path_key) {
            continue;
        }

        let (size, file_count) = if let Some(t) = tree {
            if let Some(node_id) = t.find_by_path(path) {
                if let Some(node) = t.node(node_id) {
                    (node.size, node.file_count)
                } else {
                    get_quick_path_size(path)
                }
            } else {
                get_quick_path_size(path)
            }
        } else {
            get_quick_path_size(path)
        };

        if size > 0 || file_count > 0 {
            artifacts.push(DevArtifact {
                id: format!("{:?}_{}", ecosystem, path.display()),
                ecosystem,
                kind,
                name: name.to_string(),
                project_name: "Global Environment".to_string(),
                path: path.clone(),
                size,
                file_count,
                is_dormant: false,
                days_inactive: 0,
                risk_level: risk,
                description: desc.to_string(),
                cleanup_command: cmd.to_string(),
            });
        }
    }
}

/// Fallback quick on-disk size calculator for file or directory.
fn get_quick_path_size(path: &Path) -> (u64, u64) {
    if path.is_file() {
        if let Ok(meta) = path.metadata() {
            return (meta.len(), 1);
        }
        return (0, 0);
    }

    let mut total_size = 0u64;
    let mut file_count = 0u64;

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten().take(500) {
            let p = entry.path();
            if p.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total_size += meta.len();
                    file_count += 1;
                }
            } else if p.is_dir() {
                if let Ok(sub_entries) = std::fs::read_dir(&p) {
                    for sub in sub_entries.flatten().take(200) {
                        if let Ok(meta) = sub.metadata() {
                            if meta.is_file() {
                                total_size += meta.len();
                                file_count += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    (total_size, file_count)
}

/// Safely cleans a developer artifact directory or cache using Windows Recycle Bin guardrails (ADR-015 §4).
pub fn clean_dev_artifact(path: &Path, dry_run: bool, send_to_recycle_bin: bool) -> CleanupReport {
    // 1. Immutable safety guardrail
    if is_path_protected(path) {
        return CleanupReport {
            files_failed: 1,
            is_dry_run: dry_run,
            ..Default::default()
        };
    }

    // 2. Reject attempts to delete source code or git repos
    let path_str = path.to_string_lossy().to_ascii_lowercase();
    if path_str.ends_with(".git")
        || path_str.ends_with(".rs")
        || path_str.ends_with(".cs")
        || path_str.ends_with(".js")
        || path_str.ends_with(".py")
        || path_str.ends_with("cargo.toml")
        || path_str.ends_with("package.json")
    {
        return CleanupReport {
            files_failed: 1,
            is_dry_run: dry_run,
            ..Default::default()
        };
    }

    // 3. Execute safe deletion via Recycle Bin COM API
    execute_cleanup(&[path.to_path_buf()], dry_run, send_to_recycle_bin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{FileAttributeFlags, NodeId, ScanEvent};
    use std::ffi::OsString;
    use tempfile::tempdir;

    fn dir_meta(name: &str) -> common::DirMetadata {
        common::DirMetadata {
            name: OsString::from(name),
            is_reparse_point: false,
        }
    }

    fn file_meta(name: &str, size: u64) -> common::FileMetadata {
        common::FileMetadata {
            name: OsString::from(name),
            size,
            extension: Some("o".to_string()),
            modified: None,
            created: None,
            attributes: FileAttributeFlags::default(),
        }
    }

    #[test]
    fn detect_dev_artifacts_from_tree() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta(r"C:\Projects\MyRustApp"),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: dir_meta("target"),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(1),
                meta: file_meta("libapp.rlib", 10485760), // 10 MB
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(1),
                total_size: 10485760,
                file_count: 1,
                dir_count: 0,
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 10485760,
                file_count: 1,
                dir_count: 1,
            },
        ];

        let tree = StorageTree::build(events).unwrap();
        let artifacts = detect_dev_artifacts(Some(&tree));

        assert!(!artifacts.is_empty());
        let target_art = artifacts.iter().find(|a| a.name == "target");
        assert!(target_art.is_some());
        let art = target_art.unwrap();
        assert_eq!(art.ecosystem, DevEcosystem::Rust);
        assert_eq!(art.kind, DevArtifactKind::ProjectArtifact);
        assert_eq!(art.size, 10485760);
        assert_eq!(art.file_count, 1);
        assert_eq!(art.project_name, "MyRustApp");
    }

    #[test]
    fn clean_dev_artifact_guards_protected_and_git_paths() {
        let git_dir = Path::new(r"C:\Projects\App\.git");
        let rep = clean_dev_artifact(git_dir, false, true);
        assert_eq!(rep.files_failed, 1);
        assert_eq!(rep.files_reclaimed, 0);

        let sys_dir = Path::new(r"C:\Windows\System32");
        let sys_rep = clean_dev_artifact(sys_dir, false, true);
        assert_eq!(sys_rep.files_failed, 1);
        assert_eq!(sys_rep.files_reclaimed, 0);
    }

    #[test]
    fn clean_dev_artifact_dry_run_on_temp_target() {
        let dir = tempdir().unwrap();
        let target_dir = dir.path().join("target");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(target_dir.join("build.log"), b"compile log").unwrap();

        let report = clean_dev_artifact(&target_dir, true, true);
        assert!(report.is_dry_run);
        assert_eq!(report.files_failed, 0);
        assert!(target_dir.exists());
    }
}
