# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0] - 2026-10-07

### Added
- **Core Filesystem Scanner (`scanner` crate)**:
  - High-performance parallel traversal utilizing Rayon and Win32 NTFS APIs (`FindFirstFileExW`).
  - Allocation-accurate sizing (4KB cluster alignment) for physical on-disk measurement.
  - Reparse point boundary guards (junctions and symlinks are never traversed).
  - Throttled real-time progress callbacks for file and byte counts.
- **In-Memory Arena Storage Tree (`storage-tree` crate)**:
  - Cache-friendly arena representation (`Vec<TreeNode>`) supporting $\ge 1,000,000$ nodes in $< 55\text{ MB}$ RSS.
  - Constant-time $O(1)$ random lookups by `NodeId`.
  - Subtree size aggregation and top-N file queries via binary min-heap.
- **Direct2D Hardware-Accelerated Treemap Visualization (`app` + `storage-tree`)**:
  - Squarified treemap layout engine in Rust core.
  - Win2D `CanvasControl` rendering at 60/120 FPS with Mica window styling.
  - Interactive drill-down, ancestor breadcrumb bar, floating hover tooltips, and file inspector pane.
- **Sub-5ms In-Memory Search Engine**:
  - Tokenized query parser supporting `ext:`, `size:`, and `type:` filters.
  - Integrated with WinUI 3 `AutoSuggestBox` with direct treemap navigation.
- **Safe Storage Cleanup Engine**:
  - Deterministic rule engine for crash dumps, stale logs, thumbnail caches, and Windows temp files.
  - Immutable system blacklist guarding Windows OS and boot paths.
  - Dry-run simulation mode and safe routing through Windows Recycle Bin via COM.
- **Application Manager & Storage Attribution**:
  - Win32 Registry and MSIX package discovery.
  - True cluster-allocated disk usage attribution mapped from `StorageTree`.
  - Vendor uninstaller execution and orphaned leftover scanning.
- **Developer Storage Intelligence**:
  - Deep attribution across 8 ecosystems: Node.js (`node_modules`, npm, pnpm, yarn), Rust (`target/`, cargo cache), .NET (`bin/`, `obj/`, nuget), Python (`venv`, `__pycache__`), Java (`.gradle`, `.m2`), Docker/WSL (VHDX, buildkit, volumes), Git (`.git/objects/pack`), and IDE caches.
  - Dormancy detection (>30 days since last modification).
  - Immutable safety guardrails protecting project source code and git repositories.
- **Incremental Indexing & USN Synchronization**:
  - Embedded SQLite database in WAL mode with sub-100ms tree hydration.
  - Dual-tier change synchronization using NTFS USN Change Journal (`FSCTL_READ_USN_JOURNAL`) with directory timestamp differential fallback.
- **Zero-Crash C-ABI Bridge (`ffi` crate)**:
  - Unmanaged C ABI exports with `csbindgen` generated P/Invoke signatures.
  - Panic guards on every export catching unhandled native panics safely.
  - Complete `SafeHandle` lifecycle management in managed C# code.
