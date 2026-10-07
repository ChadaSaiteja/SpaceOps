//! Work-stealing parallel directory walk (Sub-phase 2.5, ADR-007) with progress
//! throttling and cancellation (Sub-phase 2.6), built on the reparse-point
//! classification + on-disk sizing (2.3) and error tiering (2.4, ADR-004).
//!
//! Type definitions (`ScanEvent`, `ScanSummary`, `ScanProgress`) live in `common`
//! (ADR-009 #4); this module re-exports them.

use crate::metadata;
use common::{CancellationToken, InaccessibleReason, NodeId, ScanError};
use rayon::prelude::*;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const ERROR_SHARING_VIOLATION: i32 = 32;

// Re-export types from common so existing `use scanner::traversal::*` paths still work.
pub use common::{ScanEvent, ScanProgress, ScanSummary};

/// Monotonic, thread-safe NodeId allocator shared across worker threads.
struct IdAllocator(AtomicU64);

impl IdAllocator {
    fn next(&self) -> NodeId {
        NodeId(self.0.fetch_add(1, Ordering::Relaxed))
    }
}

/// Thread-safe event collector. A Mutex is sufficient here: events are appended in
/// short bursts relative to the surrounding filesystem I/O, so contention is minor.
struct EventSink(Mutex<Vec<ScanEvent>>);

impl EventSink {
    fn push(&self, event: ScanEvent) {
        self.0.lock().unwrap().push(event);
    }

    fn into_inner(self) -> Vec<ScanEvent> {
        self.0.into_inner().unwrap()
    }
}

const ERROR_ACCESS_DENIED: i32 = 5;
const HRESULT_ACCESS_DENIED: i32 = -2147024891; // 0x80070005

fn classify_io_error(err: &io::Error) -> InaccessibleReason {
    if err.kind() == io::ErrorKind::PermissionDenied {
        return InaccessibleReason::PermissionDenied;
    }
    match err.raw_os_error() {
        Some(ERROR_SHARING_VIOLATION) => InaccessibleReason::Locked,
        Some(ERROR_ACCESS_DENIED) | Some(HRESULT_ACCESS_DENIED) => {
            InaccessibleReason::PermissionDenied
        }
        _ => InaccessibleReason::Other(err.to_string()),
    }
}

/// All state shared across worker threads for a single scan, bundled to keep
/// walk_dir/process_entry signatures manageable.
struct ScanContext<'a> {
    cluster_size: u64,
    ids: IdAllocator,
    events: EventSink,
    inaccessible_count: AtomicU64,
    files_scanned: AtomicU64,
    bytes_scanned: AtomicU64,
    progress_interval: Duration,
    max_depth: Option<usize>,
    last_progress_at: Mutex<Instant>,
    cancel: &'a CancellationToken,
    on_progress: &'a (dyn Fn(ScanProgress) + Send + Sync),
}

impl ScanContext<'_> {
    fn maybe_emit_progress(&self, current_path: &Path) {
        let mut last = self.last_progress_at.lock().unwrap();
        if last.elapsed() < self.progress_interval {
            return;
        }
        *last = Instant::now();
        drop(last);

        (self.on_progress)(ScanProgress {
            files_scanned: self.files_scanned.load(Ordering::Relaxed),
            bytes_scanned: self.bytes_scanned.load(Ordering::Relaxed),
            current_path: current_path.to_path_buf(),
        });
    }
}

/// Baseline full-depth scan. Delegates to `scan_with_options` with no depth limit.
pub fn scan(
    root: &Path,
    cancel: &CancellationToken,
    progress_interval: Duration,
    on_progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<(Vec<ScanEvent>, ScanSummary), ScanError> {
    scan_with_options(root, cancel, progress_interval, None, on_progress)
}

/// Scans with optional depth limiting (`max_depth: Some(1)` for instant shallow top-level scan).
pub fn scan_with_options(
    root: &Path,
    cancel: &CancellationToken,
    progress_interval: Duration,
    max_depth: Option<usize>,
    on_progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<(Vec<ScanEvent>, ScanSummary), ScanError> {
    let root_meta = std::fs::symlink_metadata(root)
        .map_err(|_| ScanError::InvalidPath(root.to_path_buf()))?;
    if !root_meta.is_dir() {
        return Err(ScanError::InvalidPath(root.to_path_buf()));
    }

    let cluster_size =
        crate::winfs::cluster_size(root).map_err(|_| ScanError::DeviceLost(root.to_path_buf()))?;

    let ctx = ScanContext {
        cluster_size,
        ids: IdAllocator(AtomicU64::new(0)),
        events: EventSink(Mutex::new(Vec::new())),
        inaccessible_count: AtomicU64::new(0),
        files_scanned: AtomicU64::new(0),
        bytes_scanned: AtomicU64::new(0),
        progress_interval,
        max_depth,
        last_progress_at: Mutex::new(Instant::now()),
        cancel,
        on_progress,
    };

    let root_id = ctx.ids.next();
    ctx.events.push(ScanEvent::EnteredDirectory {
        parent_id: None,
        id: root_id,
        meta: common::DirMetadata {
            name: root.as_os_str().to_os_string(),
            is_reparse_point: false,
        },
    });

    let (total_size, file_count, dir_count) = walk_dir(root, root_id, 0, &ctx);

    if cancel.is_cancelled() {
        return Err(ScanError::Cancelled);
    }

    ctx.events.push(ScanEvent::DirectoryComplete {
        id: root_id,
        total_size,
        file_count,
        dir_count,
    });

    let summary = ScanSummary {
        total_files: file_count,
        total_dirs: dir_count + 1, // + root itself
        total_size,
        inaccessible_count: ctx.inaccessible_count.load(Ordering::Relaxed),
    };

    Ok((ctx.events.into_inner(), summary))
}

/// Enumerates one directory's entries in a single batch using `read_directory_fast`,
/// extracting all metadata from `WIN32_FIND_DATAW` without redundant syscalls,
/// then fans them out across rayon's work-stealing pool.
fn walk_dir(
    dir: &Path,
    dir_id: NodeId,
    current_depth: usize,
    ctx: &ScanContext,
) -> (u64, u64, u64) {
    if ctx.cancel.is_cancelled() {
        return (0, 0, 0);
    }

    let entries = match crate::winfs::read_directory_fast(dir) {
        Ok(e) => e,
        Err(err) => {
            ctx.events.push(ScanEvent::Inaccessible {
                parent_id: dir_id,
                path: dir.to_path_buf(),
                reason: classify_io_error(&err),
            });
            ctx.inaccessible_count.fetch_add(1, Ordering::Relaxed);
            return (0, 0, 0);
        }
    };

    entries
        .into_par_iter()
        .map(|entry| process_fast_entry(&entry, dir_id, current_depth, ctx))
        .reduce(
            || (0u64, 0u64, 0u64),
            |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2),
        )
}

fn process_fast_entry(
    entry: &crate::winfs::FastDirEntry,
    dir_id: NodeId,
    current_depth: usize,
    ctx: &ScanContext,
) -> (u64, u64, u64) {
    if ctx.cancel.is_cancelled() {
        return (0, 0, 0);
    }

    if entry.is_reparse_point {
        // Junction/symlink/mount point: shown as a leaf, never followed (ADR-008 #1).
        let child_id = ctx.ids.next();
        ctx.events.push(ScanEvent::EnteredDirectory {
            parent_id: Some(dir_id),
            id: child_id,
            meta: metadata::dir_metadata_from_fast_entry(entry),
        });
        ctx.events.push(ScanEvent::DirectoryComplete {
            id: child_id,
            total_size: 0,
            file_count: 0,
            dir_count: 0,
        });
        return (0, 0, 1);
    }

    if entry.is_directory {
        let child_id = ctx.ids.next();
        ctx.events.push(ScanEvent::EnteredDirectory {
            parent_id: Some(dir_id),
            id: child_id,
            meta: metadata::dir_metadata_from_fast_entry(entry),
        });

        // Depth limiting check (for instant shallow scan)
        if ctx.max_depth.map_or(false, |limit| current_depth + 1 >= limit) {
            ctx.events.push(ScanEvent::DirectoryComplete {
                id: child_id,
                total_size: 0,
                file_count: 0,
                dir_count: 0,
            });
            return (0, 0, 1);
        }

        let (child_size, child_files, child_dirs) =
            walk_dir(&entry.path, child_id, current_depth + 1, ctx);
        ctx.events.push(ScanEvent::DirectoryComplete {
            id: child_id,
            total_size: child_size,
            file_count: child_files,
            dir_count: child_dirs,
        });

        (child_size, child_files, child_dirs + 1)
    } else {
        match metadata::file_metadata_from_fast_entry(entry, ctx.cluster_size) {
            Ok(file_meta) => {
                let size = file_meta.size;
                ctx.events.push(ScanEvent::FileFound {
                    parent_id: dir_id,
                    meta: file_meta,
                });
                ctx.files_scanned.fetch_add(1, Ordering::Relaxed);
                ctx.bytes_scanned.fetch_add(size, Ordering::Relaxed);
                ctx.maybe_emit_progress(&entry.path);
                (size, 1, 0)
            }
            Err(err) => {
                ctx.events.push(ScanEvent::Inaccessible {
                    parent_id: dir_id,
                    path: entry.path.clone(),
                    reason: classify_io_error(&err),
                });
                ctx.inaccessible_count.fetch_add(1, Ordering::Relaxed);
                (0, 0, 0)
            }
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::windows::fs::symlink_dir;
    use tempfile::tempdir;

    /// Scans with a fresh, never-cancelled token and an immediate (zero-throttle)
    /// no-op progress callback — convenient default for tests that don't care about
    /// progress/cancellation specifically.
    fn scan_default(root: &Path) -> Result<(Vec<ScanEvent>, ScanSummary), ScanError> {
        scan(root, &CancellationToken::new(), Duration::ZERO, &|_| {})
    }

    #[test]
    fn walks_flat_directory() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("a.txt"), b"hello").unwrap();
        fs::write(root.path().join("b.txt"), b"world!").unwrap();

        let (_, summary) = scan_default(root.path()).unwrap();

        assert_eq!(summary.total_files, 2);
        assert_eq!(summary.total_dirs, 1);
        assert_eq!(summary.inaccessible_count, 0);
        assert!(summary.total_size >= 11);
    }

    #[test]
    fn walks_nested_directories() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("sub1")).unwrap();
        fs::create_dir(root.path().join("sub1").join("sub2")).unwrap();
        fs::write(root.path().join("top.txt"), b"12345").unwrap();
        fs::write(root.path().join("sub1").join("mid.txt"), b"1234567890").unwrap();
        fs::write(
            root.path().join("sub1").join("sub2").join("deep.txt"),
            b"123",
        )
        .unwrap();

        let (events, summary) = scan_default(root.path()).unwrap();

        assert_eq!(summary.total_files, 3);
        assert_eq!(summary.total_dirs, 3);

        let root_complete = events
            .iter()
            .find_map(|e| match e {
                ScanEvent::DirectoryComplete {
                    id,
                    file_count,
                    dir_count,
                    ..
                } if *id == NodeId(0) => Some((*file_count, *dir_count)),
                _ => None,
            })
            .expect("root DirectoryComplete event present");
        assert_eq!(root_complete, (3, 2));
    }

    #[test]
    fn empty_directory_has_no_events_beyond_enter_and_complete() {
        let root = tempdir().unwrap();
        let (events, summary) = scan_default(root.path()).unwrap();

        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.total_dirs, 1);
        assert_eq!(summary.total_size, 0);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn directory_junction_is_a_leaf_and_not_traversed() {
        let root = tempdir().unwrap();
        let target = root.path().join("target_dir");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("inside.txt"), b"should not be counted").unwrap();

        let link = root.path().join("link_dir");
        if symlink_dir(&target, &link).is_err() {
            eprintln!("skipping: creating a directory symlink requires Developer Mode or admin");
            return;
        }

        let (events, summary) = scan_default(root.path()).unwrap();

        assert_eq!(summary.total_files, 0);
        assert!(events.iter().any(|e| matches!(
            e,
            ScanEvent::EnteredDirectory { meta, .. } if meta.is_reparse_point
        )));
    }

    #[test]
    fn invalid_root_is_a_fatal_error() {
        let missing = std::env::temp_dir().join("this_path_should_not_exist_12345");
        let result = scan_default(&missing);
        assert!(matches!(result, Err(ScanError::InvalidPath(_))));
    }

    #[test]
    fn classifies_permission_denied() {
        let err = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(classify_io_error(&err), InaccessibleReason::PermissionDenied);
    }

    #[test]
    fn classifies_sharing_violation_as_locked() {
        let err = io::Error::from_raw_os_error(ERROR_SHARING_VIOLATION);
        assert_eq!(classify_io_error(&err), InaccessibleReason::Locked);
    }

    #[test]
    fn classifies_unrecognized_error_as_other() {
        let err = io::Error::from_raw_os_error(999_999);
        assert!(matches!(
            classify_io_error(&err),
            InaccessibleReason::Other(_)
        ));
    }

    #[test]
    fn inaccessible_subdirectory_does_not_abort_scan() {
        let root = tempdir().unwrap();
        let blocked = root.path().join("blocked");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("secret.txt"), b"hidden").unwrap();
        fs::write(root.path().join("visible.txt"), b"ok").unwrap();

        let username = std::env::var("USERNAME").unwrap_or_default();
        let applied = std::process::Command::new("icacls")
            .arg(&blocked)
            .arg("/deny")
            .arg(format!("{}:(R)", username))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !applied {
            eprintln!("skipping: could not apply a deny ACE via icacls in this environment");
            return;
        }

        let (events, summary) = scan_default(root.path()).unwrap();

        // Always restore access so tempdir cleanup can remove the directory afterward.
        let _ = std::process::Command::new("icacls")
            .arg(&blocked)
            .arg("/remove:d")
            .arg(&username)
            .status();

        if summary.inaccessible_count == 0 {
            eprintln!(
                "skipping assertions: deny ACE did not block access in this environment \
                 (e.g. elevated/service account bypassing ACLs)"
            );
            return;
        }

        assert_eq!(summary.total_files, 1); // visible.txt only
        assert!(events.iter().any(|e| matches!(
            e,
            ScanEvent::Inaccessible {
                reason: InaccessibleReason::PermissionDenied,
                ..
            }
        )));
    }

    #[test]
    fn parallel_walk_aggregates_correctly_on_a_larger_tree() {
        // Stresses the work-stealing path across many directories/files to catch races
        // in the atomic NodeId allocator and the Mutex-guarded event sink.
        let root = tempdir().unwrap();
        let dirs = 50u64;
        let files_per_dir = 20u64;
        let bytes_per_file = 7u64;

        for d in 0..dirs {
            let sub = root.path().join(format!("dir{d}"));
            fs::create_dir(&sub).unwrap();
            for f in 0..files_per_dir {
                fs::write(
                    sub.join(format!("file{f}.bin")),
                    vec![b'x'; bytes_per_file as usize],
                )
                .unwrap();
            }
        }

        let (_, summary) = scan_default(root.path()).unwrap();

        assert_eq!(summary.total_files, dirs * files_per_dir);
        assert_eq!(summary.total_dirs, dirs + 1); // + root
        assert_eq!(summary.inaccessible_count, 0);
        assert!(summary.total_size >= dirs * files_per_dir * bytes_per_file);
    }

    #[test]
    fn cancellation_before_scan_starts_returns_cancelled_error() {
        let root = tempdir().unwrap();
        for d in 0..20 {
            let sub = root.path().join(format!("dir{d}"));
            fs::create_dir(&sub).unwrap();
            fs::write(sub.join("f.txt"), b"data").unwrap();
        }

        let cancel = CancellationToken::new();
        cancel.cancel(); // cancelled before scan() is even called

        let result = scan(root.path(), &cancel, Duration::ZERO, &|_| {});
        assert!(matches!(result, Err(ScanError::Cancelled)));
    }

    #[test]
    fn cancellation_mid_scan_halts_and_returns_cancelled_error() {
        let root = tempdir().unwrap();
        for d in 0..200 {
            let sub = root.path().join(format!("dir{d}"));
            fs::create_dir(&sub).unwrap();
            for f in 0..20 {
                fs::write(sub.join(format!("f{f}.txt")), b"data").unwrap();
            }
        }

        let cancel = CancellationToken::new();
        let cancel_for_callback = cancel.clone();
        // Cancel as soon as the first progress tick fires, simulating a user clicking
        // "Cancel" partway through a real scan.
        let result = scan(
            root.path(),
            &cancel,
            Duration::ZERO,
            &move |_progress| cancel_for_callback.cancel(),
        );

        assert!(matches!(result, Err(ScanError::Cancelled)));
    }

    #[test]
    fn progress_callback_fires_and_reports_increasing_counts() {
        let root = tempdir().unwrap();
        for f in 0..10 {
            fs::write(root.path().join(format!("f{f}.txt")), b"12345").unwrap();
        }

        let seen_files_scanned = Mutex::new(Vec::new());
        let result = scan(
            root.path(),
            &CancellationToken::new(),
            Duration::ZERO, // no throttling: fire on every file, for a deterministic test
            &|p: ScanProgress| seen_files_scanned.lock().unwrap().push(p.files_scanned),
        );

        assert!(result.is_ok());
        let seen = seen_files_scanned.into_inner().unwrap();
        assert_eq!(seen.len(), 10); // one callback per file, since throttle is zero
        assert_eq!(*seen.iter().max().unwrap(), 10);
    }

    #[test]
    fn shallow_scan_limits_depth_and_does_not_traverse_deep_children() {
        let root = tempdir().unwrap();
        let sub = root.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(root.path().join("root_file.txt"), b"root content").unwrap();
        fs::write(sub.join("deep_file.txt"), b"deep content").unwrap();

        // max_depth: Some(1) -> only inspect immediate children of root
        let (events, summary) = scan_with_options(
            root.path(),
            &CancellationToken::new(),
            Duration::ZERO,
            Some(1),
            &|_| {},
        )
        .unwrap();

        assert_eq!(summary.total_files, 1);
        assert_eq!(summary.total_dirs, 2); // root + sub
        assert!(!events.iter().any(|e| matches!(
            e,
            ScanEvent::FileFound { meta, .. } if meta.name == "deep_file.txt"
        )));
    }
}


