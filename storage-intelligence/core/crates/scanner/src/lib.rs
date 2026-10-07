//! Filesystem traversal and measurement (ADR-003). Pure Rust, no FFI/unsafe
//! beyond the Win32 calls confined to `winfs` (justified: no std equivalent exists).
//! Design: docs/components/01-filesystem-scanner-design.md

pub mod mft;
pub mod metadata;
pub mod traversal;
pub mod winfs;

pub use metadata::{DirMetadata, FileAttributeFlags, FileMetadata};
pub use traversal::{scan, scan_with_options, ScanEvent, ScanProgress, ScanSummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanEngineStrategy {
    Auto,
    MftPreferred,
    Win32Only,
    ShallowDepth(usize),
}

/// Resilient multi-tier scanner supervisor (ADR-011).
///
/// 1. If `Auto` or `MftPreferred` on a whole NTFS drive: attempts Tier 1 direct MFT read.
/// 2. If MFT encounters any mid-scan errors (privileges, locked drive, short read):
///    cleanly drops all partial MFT allocations and transparently restarts with Tier 2
///    Win32 kernel-buffered parallel traversal under a fresh state.
/// 3. If `ShallowDepth(d)`: runs instant shallow scan bounded to depth `d`.
pub fn scan_volume_resilient(
    root: &std::path::Path,
    strategy: ScanEngineStrategy,
    cancel: &common::CancellationToken,
    progress_interval: std::time::Duration,
    on_progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<(Vec<ScanEvent>, ScanSummary), common::ScanError> {
    match strategy {
        ScanEngineStrategy::ShallowDepth(depth) => {
            traversal::scan_with_options(root, cancel, progress_interval, Some(depth), on_progress)
        }
        ScanEngineStrategy::Win32Only => {
            traversal::scan_with_options(root, cancel, progress_interval, None, on_progress)
        }
        ScanEngineStrategy::Auto | ScanEngineStrategy::MftPreferred => {
            let is_whole_drive = mft::volume::extract_drive_letter(root).is_some()
                && root.parent().is_none();
            let is_ntfs = mft::volume::is_ntfs_volume(root);

            if is_whole_drive && is_ntfs {
                match mft::scan_mft(root, cancel, progress_interval, on_progress) {
                    Ok((events, summary, _high_usn)) => return Ok((events, summary)),
                    Err(mft::MftScanError::Fatal(e)) => return Err(e),
                    Err(mft::MftScanError::MidScanFailure(_reason)) => {
                        // ADR-011 Clean Discard-and-Restart Fallback:
                        // Discard partial MFT state completely and restart via Win32.
                    }
                }
            }

            traversal::scan_with_options(root, cancel, progress_interval, None, on_progress)
        }
    }
}

