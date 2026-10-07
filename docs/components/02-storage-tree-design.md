# Storage Tree — Component Design

Status: **Approved. Implementation phase plan follows below.**

Governed by: ADR-001 (FFI), ADR-003 (module boundaries), ADR-004 (errors), ADR-007 (threading), ADR-008 #4 (scanner streams → tree assembles), ADR-009 (storage tree decisions).

## 1. Problem

Given a flat stream of `ScanEvent`s produced by the scanner (Phase 2), build an efficient in-memory hierarchical tree structure that the UI/treemap can query interactively — without bulk-marshaling millions of nodes across the FFI boundary (ADR-001 §2).

## 2. Requirements

- Build a tree from `Vec<ScanEvent>`, preserving parent→child relationships (ADR-008 #4).
- Each node stores: id, name, size (on-disk, aggregated for dirs), file/dir counts, extension, modified/created time, attributes, node type (dir/file/reparse-point/inaccessible) (PRD §6.C).
- Query API: children of node (paginated, sorted by size desc), node info, ancestors, top-N files by size, root id, full path reconstruction.
- Handle ≥1M nodes within ~300 MB RSS for the tree alone.
- Tree build ≤ scan time; query latency < 1ms for interactive use.
- Immutable post-build; thread-safe for concurrent reads (ADR-009 #2).
- In-memory only at MVP — no SQLite persistence (ADR-009 #3).

## 3. Architecture

Arena-backed flat `Vec<TreeNode>` indexed by `NodeId.0`, with a parallel `Vec<Vec<NodeId>>` for children (ADR-009 #1). Children pre-sorted by size descending during build.

## 4. Dependencies

- `ScanEvent`, `FileMetadata`, `DirMetadata`, `FileAttributeFlags` relocated to `common` crate (ADR-009 #4).
- Consumed by `ffi` via two-handle lifecycle: `ScanResultHandle` → `TreeHandle` (ADR-009 #5).

## 5. Interfaces

See approved design document for full Rust API (`StorageTree` struct), FFI API (`tree_create`, `tree_destroy`, query functions), and C# wrapper sketch.

## 6. Testing Strategy

- Unit tests in `storage-tree` crate (pure Rust): tree build, aggregation, queries, edge cases.
- FFI integration tests: handle lifecycle, pagination, error codes.
- Performance benchmark: 1M synthetic events.

## 7. Performance Targets

Measured via `cargo run --release -p storage-tree --example benchmark`.

## 8. Explicitly Out of Scope

SQLite persistence (Phase 9), incremental rescan (Phase 9), file categorization (Phase 6), treemap layout (Phase 4), search index (Phase 5), duplicate detection (Phase 15).
