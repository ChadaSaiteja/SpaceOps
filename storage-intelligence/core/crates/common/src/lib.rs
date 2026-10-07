//! Shared types used across the Rust core (ADR-003). No internal workspace dependencies.
//!
//! Types relocated here from `scanner` per ADR-009 #4 so that both `scanner` and
//! `storage-tree` can depend on `common` only, preserving ADR-003's strict layering.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

/// Stable identity for a node within a single scan (not persisted across scans).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);

/// Shared, cloneable cancellation flag checked by scanner worker threads (ADR-007).
#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Per-item issue that does not abort a scan (ADR-004 tier 1).
#[derive(Debug, Clone)]
pub struct InaccessiblePath {
    pub path: PathBuf,
    pub reason: InaccessibleReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InaccessibleReason {
    PermissionDenied,
    Locked,
    BrokenReparsePoint,
    /// Fallback for OS errors not classified into a specific reason above.
    Other(String),
}

/// Fatal errors that abort a scan (ADR-004 tier 2/3).
#[derive(thiserror::Error, Debug)]
pub enum ScanError {
    #[error("invalid path: {0}")]
    InvalidPath(PathBuf),
    #[error("scan cancelled")]
    Cancelled,
    #[error("device lost during scan: {0}")]
    DeviceLost(PathBuf),
    #[error("internal error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// Scanner data types (relocated from `scanner` crate per ADR-009 #4)
// ---------------------------------------------------------------------------

/// Boolean flags extracted from Win32 `FILE_ATTRIBUTE_*` bitmask.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileAttributeFlags {
    pub readonly: bool,
    pub hidden: bool,
    pub system: bool,
    pub reparse_point: bool,
    pub compressed: bool,
    pub sparse: bool,
}

/// Per-file metadata collected during a scan.
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub name: OsString,
    /// On-disk (allocated) size (ADR-008 #2), not logical length.
    pub size: u64,
    pub extension: Option<String>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub attributes: FileAttributeFlags,
}

/// Per-directory metadata collected during a scan.
#[derive(Debug, Clone)]
pub struct DirMetadata {
    pub name: OsString,
    /// True for junctions/symlinks/mount points: shown as a leaf, never traversed (ADR-008 #1).
    pub is_reparse_point: bool,
}

/// Flat event emitted by the scanner during traversal (ADR-008 #4).
/// The storage-tree phase (Phase 3) assembles these into a hierarchical tree.
#[derive(Debug, Clone)]
pub enum ScanEvent {
    EnteredDirectory {
        parent_id: Option<NodeId>,
        id: NodeId,
        meta: DirMetadata,
    },
    FileFound {
        parent_id: NodeId,
        meta: FileMetadata,
    },
    DirectoryComplete {
        id: NodeId,
        total_size: u64,
        file_count: u64,
        dir_count: u64,
    },
    /// Per-item issue (ADR-004 tier 1): recorded, never aborts the scan.
    Inaccessible {
        parent_id: NodeId,
        path: PathBuf,
        reason: InaccessibleReason,
    },
}

/// Aggregated progress snapshot, emitted at most once per progress interval (throttled,
/// not per-file) so callbacks (and any UI-thread dispatch on the C# side) aren't flooded.
#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub current_path: PathBuf,
}

/// Final aggregate summary of a completed scan.
#[derive(Debug, Clone, Default)]
pub struct ScanSummary {
    pub total_files: u64,
    pub total_dirs: u64,
    pub total_size: u64,
    pub inaccessible_count: u64,
}
