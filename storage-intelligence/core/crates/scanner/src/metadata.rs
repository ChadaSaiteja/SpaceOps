//! Per-entry metadata extraction, including on-disk size and reparse-point
//! classification (ADR-008 #1, #2).
//!
//! Type definitions (`FileMetadata`, `DirMetadata`, `FileAttributeFlags`) live in
//! `common` (ADR-009 #4); this module re-exports them and provides the construction
//! functions that depend on Windows-specific logic.

use crate::winfs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

// Re-export types from common so existing `use scanner::metadata::*` paths still work.
pub use common::{DirMetadata, FileAttributeFlags, FileMetadata};

const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;

/// Construct `FileAttributeFlags` from the raw Win32 bitmask.
pub fn flags_from_raw(raw: u32) -> FileAttributeFlags {
    FileAttributeFlags {
        readonly: raw & FILE_ATTRIBUTE_READONLY != 0,
        hidden: raw & FILE_ATTRIBUTE_HIDDEN != 0,
        system: raw & FILE_ATTRIBUTE_SYSTEM != 0,
        reparse_point: raw & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        compressed: raw & FILE_ATTRIBUTE_COMPRESSED != 0,
        sparse: raw & FILE_ATTRIBUTE_SPARSE_FILE != 0,
    }
}

/// Metadata for a regular (non-reparse-point) file. `symlink_metadata` must have
/// already confirmed this entry is not a reparse point before calling this.
pub fn file_metadata(
    path: &Path,
    meta: &std::fs::Metadata,
    cluster_size: u64,
) -> std::io::Result<FileMetadata> {
    let attributes = flags_from_raw(meta.file_attributes());

    let size = if attributes.compressed || attributes.sparse {
        winfs::compressed_file_size(path)?
    } else {
        winfs::round_up_to_cluster(meta.len(), cluster_size)
    };

    Ok(FileMetadata {
        name: path.file_name().unwrap_or_default().to_os_string(),
        size,
        extension: path
            .extension()
            .map(|ext| ext.to_string_lossy().into_owned()),
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        attributes,
    })
}

/// Metadata for a directory entry. `is_reparse_point` is precomputed by the caller
/// from `symlink_metadata` (junctions/symlinks/mount points are never followed).
pub fn dir_metadata(path: &Path, is_reparse_point: bool) -> DirMetadata {
    DirMetadata {
        name: path.file_name().unwrap_or_default().to_os_string(),
        is_reparse_point,
    }
}

/// Metadata extracted directly from a `FastDirEntry` without any redundant syscalls.
pub fn file_metadata_from_fast_entry(
    entry: &winfs::FastDirEntry,
    cluster_size: u64,
) -> std::io::Result<FileMetadata> {
    let attributes = flags_from_raw(entry.attributes);

    let size = if attributes.compressed || attributes.sparse {
        winfs::compressed_file_size(&entry.path)?
    } else {
        winfs::round_up_to_cluster(entry.logical_size, cluster_size)
    };

    Ok(FileMetadata {
        name: entry.name.clone(),
        size,
        extension: entry
            .path
            .extension()
            .map(|ext| ext.to_string_lossy().into_owned()),
        modified: entry.modified,
        created: entry.created,
        attributes,
    })
}

/// Directory metadata directly from a `FastDirEntry`.
pub fn dir_metadata_from_fast_entry(entry: &winfs::FastDirEntry) -> DirMetadata {
    DirMetadata {
        name: entry.name.clone(),
        is_reparse_point: entry.is_reparse_point,
    }
}
