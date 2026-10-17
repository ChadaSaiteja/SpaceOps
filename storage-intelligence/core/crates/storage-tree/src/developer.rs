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
        let mut candidates: Vec<Candidate> = Vec::new();

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
                    if node.size > 0 && has_project_marker(t, id, node.parent, &name_lower) {
                        candidates.push(Candidate {
                            id,
                            ecosystem,
                            kind,
                            risk,
                            desc,
                            cmd,
                        });
                    }
                }
            }
        }

        // An artifact nested inside another one (node_modules inside node_modules, a
        // package's own bin/) is part of the outer artifact, never a separate candidate.
        let candidate_ids: std::collections::HashSet<NodeId> =
            candidates.iter().map(|c| c.id).collect();

        for c in candidates {
            if has_candidate_ancestor(t, c.id, &candidate_ids) {
                continue;
            }
            let Some(node) = t.node(c.id) else { continue };
            let Some(path) = t.full_path(c.id) else {
                continue;
            };
            let path_key = path.to_string_lossy().to_ascii_lowercase();
            if is_installed_software_path(&path_key) || !seen_paths.insert(path_key) {
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

            // Directories carry no modification time, so dormancy comes from the newest
            // file inside the artifact (build outputs are rewritten on every build).
            let (is_dormant, days_inactive) =
                dormancy(newest_file_mtime(t, c.id, MAX_ACTIVITY_VISITS), now);

            artifacts.push(DevArtifact {
                id: format!("{:?}_{}", c.ecosystem, path.display()),
                ecosystem: c.ecosystem,
                kind: c.kind,
                name: node.name.to_string(),
                project_name,
                path,
                size: node.size,
                file_count: node.file_count,
                is_dormant,
                days_inactive,
                risk_level: c.risk,
                description: c.desc.to_string(),
                cleanup_command: c.cmd.to_string(),
            });
        }
    }

    // 2. Scan canonical global developer caches on Windows
    probe_global_dev_caches(&mut artifacts, &mut seen_paths, tree);

    // Sort by size descending
    artifacts.sort_by_key(|a| std::cmp::Reverse(a.size));
    artifacts
}

/// `days_inactive` value meaning "no file times available, activity unknown".
pub const DAYS_UNKNOWN: u32 = u32::MAX;

/// Upper bound on nodes inspected per artifact when looking for its newest file.
const MAX_ACTIVITY_VISITS: usize = 200_000;

struct Candidate {
    id: NodeId,
    ecosystem: DevEcosystem,
    kind: DevArtifactKind,
    risk: RiskLevel,
    desc: &'static str,
    cmd: &'static str,
}

/// True when the folder sits in a real project: the build-system file that recreates it
/// is next to it (or, for virtual environments, inside it). Without this, any folder named
/// `bin`, `target` or `node_modules` (for example `~/.cargo/bin`) would be offered.
fn has_project_marker(
    t: &StorageTree,
    id: NodeId,
    parent: Option<NodeId>,
    name_lower: &str,
) -> bool {
    let child_names = |of: NodeId| -> Vec<String> {
        t.children(of)
            .unwrap_or(&[])
            .iter()
            .filter_map(|c| t.node(*c))
            .filter(|n| n.kind == NodeKind::File)
            .map(|n| n.name.to_ascii_lowercase())
            .collect()
    };
    match name_lower {
        ".venv" | "venv" => child_names(id).iter().any(|n| n == "pyvenv.cfg"),
        "__pycache__" => child_names(id).iter().any(|n| n.ends_with(".pyc")),
        _ => {
            let Some(pid) = parent else { return false };
            let siblings = child_names(pid);
            match name_lower {
                "node_modules" => siblings.iter().any(|n| n == "package.json"),
                "target" => siblings.iter().any(|n| n == "cargo.toml"),
                "bin" | "obj" => siblings.iter().any(|n| {
                    n.ends_with(".csproj") || n.ends_with(".fsproj") || n.ends_with(".vbproj")
                }),
                ".gradle" => siblings.iter().any(|n| {
                    matches!(
                        n.as_str(),
                        "build.gradle"
                            | "build.gradle.kts"
                            | "settings.gradle"
                            | "settings.gradle.kts"
                            | "gradlew"
                    )
                }),
                _ => false,
            }
        }
    }
}

fn has_candidate_ancestor(
    t: &StorageTree,
    id: NodeId,
    candidates: &std::collections::HashSet<NodeId>,
) -> bool {
    let mut cur = t.node(id).and_then(|n| n.parent);
    while let Some(pid) = cur {
        if candidates.contains(&pid) {
            return true;
        }
        cur = t.node(pid).and_then(|n| n.parent);
    }
    false
}

/// Paths that belong to installed software or toolchains rather than a user's project.
/// Their bundled `node_modules`, `bin` or `target` folders are part of a working program.
fn is_installed_software_path(path_lower: &str) -> bool {
    const PARTS: &[&str] = &[
        r"\program files",
        r"\programdata\",
        r"\windows\",
        r"\windowsapps\",
        r"\appdata\local\programs\",
        r"\appdata\local\microsoft\",
        r"\appdata\roaming\npm\",
        r"\nvm\",
        r"\resources\app",
        r"\.cargo\",
        r"\.rustup\",
        r"\.vscode\extensions\",
        r"\.vscode-insiders\extensions\",
        r"\.cursor\extensions\",
        r"\scoop\apps\",
        r"\chocolatey\",
    ];
    PARTS.iter().any(|p| path_lower.contains(p))
}

/// Newest file modification time inside `id`, visiting at most `max_visits` nodes.
fn newest_file_mtime(t: &StorageTree, id: NodeId, max_visits: usize) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    let mut stack = vec![id];
    let mut visits = 0usize;
    while let Some(cur) = stack.pop() {
        visits += 1;
        if visits > max_visits {
            break;
        }
        let Some(node) = t.node(cur) else { continue };
        if node.kind == NodeKind::File {
            if let Some(m) = node.modified {
                newest = Some(newest.map_or(m, |n| n.max(m)));
            }
        } else if let Some(children) = t.children(cur) {
            stack.extend_from_slice(children);
        }
    }
    newest
}

/// Dormant when the newest activity is 30 or more days old; unknown activity is never dormant.
fn dormancy(newest: Option<SystemTime>, now: SystemTime) -> (bool, u32) {
    match newest {
        Some(m) => {
            let days = now
                .duration_since(m)
                .map(|d| (d.as_secs() / 86400) as u32)
                .unwrap_or(0);
            (days >= 30, days)
        }
        None => (false, DAYS_UNKNOWN),
    }
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
                days_inactive: DAYS_UNKNOWN,
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
                parent_id: NodeId(0),
                meta: file_meta("Cargo.toml", 100),
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
                total_size: 10485860,
                file_count: 2,
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

    /// Builds a tree rooted at `root` from `(relative/path/with/slashes, size, modified)` files.
    fn tree_of(root: &str, files: &[(&str, u64, Option<SystemTime>)]) -> StorageTree {
        use std::collections::BTreeMap;
        // Directory paths ("" is the root) -> id, plus their files and child dirs.
        let mut ids: BTreeMap<String, u64> = BTreeMap::new();
        ids.insert(String::new(), 0);
        let mut next = 1u64;
        let mut events = vec![ScanEvent::EnteredDirectory {
            parent_id: None,
            id: NodeId(0),
            meta: dir_meta(root),
        }];
        let mut dirs_in_order = vec![String::new()];
        for (path, _, _) in files {
            let parts: Vec<&str> = path.split('/').collect();
            let mut cur = String::new();
            for part in &parts[..parts.len() - 1] {
                let child = if cur.is_empty() {
                    part.to_string()
                } else {
                    format!("{cur}/{part}")
                };
                if !ids.contains_key(&child) {
                    ids.insert(child.clone(), next);
                    events.push(ScanEvent::EnteredDirectory {
                        parent_id: Some(NodeId(ids[&cur])),
                        id: NodeId(next),
                        meta: dir_meta(part),
                    });
                    dirs_in_order.push(child.clone());
                    next += 1;
                }
                cur = child;
            }
        }
        for (path, size, modified) in files {
            let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
            let mut meta = file_meta(name, *size);
            meta.modified = *modified;
            events.push(ScanEvent::FileFound {
                parent_id: NodeId(ids[dir]),
                meta,
            });
        }
        // Post-order completion: deepest directories first.
        dirs_in_order
            .sort_by_key(|d| std::cmp::Reverse(d.matches('/').count() + !d.is_empty() as usize));
        for d in dirs_in_order {
            let inside = |p: &str| d.is_empty() || p.starts_with(&format!("{d}/"));
            let total: u64 = files.iter().filter(|f| inside(f.0)).map(|f| f.1).sum();
            let count = files.iter().filter(|f| inside(f.0)).count() as u64;
            let sub = ids
                .keys()
                .filter(|k| !k.is_empty() && *k != &d && inside(k))
                .count() as u64;
            events.push(ScanEvent::DirectoryComplete {
                id: NodeId(ids[&d]),
                total_size: total,
                file_count: count,
                dir_count: sub,
            });
        }
        StorageTree::build(events).unwrap()
    }

    fn project_artifacts(tree: &StorageTree) -> Vec<DevArtifact> {
        detect_dev_artifacts(Some(tree))
            .into_iter()
            .filter(|a| a.kind == DevArtifactKind::ProjectArtifact)
            .collect()
    }

    #[test]
    fn folders_without_a_project_marker_are_not_artifacts() {
        // A plain "bin" (no .csproj) and a "target" (no Cargo.toml), e.g. ~/go/bin or a photo folder.
        let tree = tree_of(
            r"C:\Users\u\stuff",
            &[
                ("bin/tool.exe", 5000, None),
                ("target/notes.txt", 5000, None),
            ],
        );
        assert!(project_artifacts(&tree).is_empty());
    }

    #[test]
    fn installed_software_folders_are_never_artifacts() {
        // Bundled node_modules of an installed desktop app and Node's own global modules.
        let tree = tree_of(
            r"C:\Users\u\AppData\Local",
            &[
                ("Programs/Editor/resources/app/package.json", 10, None),
                (
                    "Programs/Editor/resources/app/node_modules/x/index.js",
                    9000,
                    None,
                ),
                ("nvm/v24.14.0/package.json", 10, None),
                ("nvm/v24.14.0/node_modules/npm/index.js", 9000, None),
            ],
        );
        assert!(project_artifacts(&tree).is_empty());
    }

    #[test]
    fn nested_artifacts_are_reported_once() {
        let tree = tree_of(
            r"C:\code\web",
            &[
                ("package.json", 10, None),
                ("node_modules/lib/package.json", 10, None),
                ("node_modules/lib/node_modules/dep/index.js", 4000, None),
                ("node_modules/lib/index.js", 4000, None),
            ],
        );
        let arts = project_artifacts(&tree);
        assert_eq!(arts.len(), 1, "{arts:?}");
        assert!(arts[0].path.ends_with("node_modules"));
        assert_eq!(
            arts[0].project_name,
            r"C:\code\web".rsplit('\\').next().unwrap()
        );
    }

    #[test]
    fn dormancy_comes_from_newest_file_inside_the_artifact() {
        let now = SystemTime::now();
        let day = std::time::Duration::from_secs(86_400);
        let old = Some(now - day * 90);
        let recent = Some(now - day * 2);
        let tree = tree_of(
            r"C:\code",
            &[
                ("old/Cargo.toml", 10, old),
                ("old/target/debug/app.exe", 9000, old),
                ("new/Cargo.toml", 10, recent),
                ("new/target/debug/app.exe", 9000, recent),
                ("unknown/Cargo.toml", 10, None),
                ("unknown/target/debug/app.exe", 9000, None),
            ],
        );
        let arts = project_artifacts(&tree);
        let by = |p: &str| arts.iter().find(|a| a.project_name == p).unwrap();
        assert!(by("old").is_dormant);
        assert!(by("old").days_inactive >= 89);
        assert!(!by("new").is_dormant);
        assert!(by("new").days_inactive <= 3);
        assert!(!by("unknown").is_dormant);
        assert_eq!(by("unknown").days_inactive, DAYS_UNKNOWN);
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
