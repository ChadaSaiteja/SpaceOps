# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.2.0] - 2026-10-23

First release with downloadable builds: a portable, self-contained zip for Windows x64 and ARM64. Unzip and run; no .NET or Windows App Runtime install needed.

### Added
- **Fluent Sidebar UI**: left navigation with Overview, Treemap, Cleanup, Developer and Applications pages, shared theme and palette.
- **Overview page**: usage by category, top 5 consumers, reclaimable-now total and quick actions.
- **One cleanup list** across system junk, dormant developer artifacts and app leftovers, with one reclaimable total; only low-risk system junk is pre-selected.
- **Treemap**: folder name headers, a working 1-3 level depth selector, and folders colored by their dominant content.
- Self-contained x64 and ARM64 release packaging with SHA-256 checksums (`docs/releasing.md`).
- Landing-page app preview, guides, sitemap, robots, social image, `llms-full.txt`, `codemeta.json`.

### Changed
- Faster core: tree build ~3x, top files 108 ms -> 35 us, search ~9x, treemap layout ~2.8x at 1M nodes; fewer scanner allocations (see `docs/performance-review.md`).
- Cleanup analysis 22 s -> 2 s on a 2.1M-item profile, so "Reclaimable now" appears seconds after a scan.
- `crates/ffi` split into one module per feature; exported symbols and generated bindings unchanged.
- CI builds and tests only what changed, never twice, and runs the C# integration tests again.

### Fixed
- **Safety**: developer artifacts are only offered from real projects (`package.json`, `Cargo.toml`, `*.csproj`, ...) and never from installed software (Program Files, `AppData\Local\Programs`, nvm, `.cargo`, app bundles).
- **Safety**: shared folders such as `AppData\Local\Programs` are never offered as app leftovers; leftovers are marked Medium risk, never pre-selected, and folders active in the last 30 days are skipped.
- Dormancy detection: it never worked (every artifact showed "Active today"); it now uses the newest file inside each artifact.
- Leftover size estimates could run effectively forever on folders with self-referencing junctions; they are now bounded and never follow junctions.
- Store app enumeration no longer freezes the Applications page when Windows is slow to answer.
- Startup crash caused by filter handlers running before the Applications and Developer pages were ready.
- Registry size estimates are labelled "Registry est." instead of "Scanned"; row text is left-aligned.

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
