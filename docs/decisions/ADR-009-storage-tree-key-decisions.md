# ADR-009: Storage Tree — Key Design Decisions

## Status

Accepted

## Context

Phase 3 (Storage Tree) design surfaced five design decisions that affect how the scanner's flat event stream is transformed into a queryable hierarchical structure, how the tree is exposed across the FFI boundary, and what persistence/incremental capabilities are in scope for MVP. These decisions build on ADR-001 (FFI/batched queries), ADR-003 (module boundaries), ADR-004 (error handling), ADR-007 (threading), and ADR-008 #4 (scanner streams flat events; tree phase assembles).

## Decisions

### 1. In-memory arena-backed tree (flat Vec, not pointer-per-node)

**Decision:** The storage tree uses a flat `Vec<TreeNode>` indexed by `NodeId.0` as a dense arena, with a parallel `Vec<Vec<NodeId>>` for parent→children relationships. No `Arc<Node>` pointer graph, no ECS-style struct-of-arrays.

**Reason:** Scanner `NodeId`s are monotonically increasing integers 0..n, mapping directly to Vec indices — O(1) lookup with zero hash overhead. Contiguous memory layout gives excellent cache locality. Memory estimate: ~200 bytes/node × 1M nodes ≈ 220 MB, well within the ~500 MB budget. Pointer-per-node (`Arc<Node>`) would scatter nodes across the heap, cause ~2-3× worse memory usage, and complicate FFI. ECS-style is overkill for tree-local query patterns.

### 2. Immutable post-build

**Decision:** The `StorageTree` is **immutable after construction**. No mutation API is exposed. C# holds a read-only `TreeHandle` and queries it; to "update," it builds a new tree from a new scan and disposes the old handle.

**Reason:** Immutability eliminates all concurrent-read/write hazards. `&StorageTree` is automatically `Send + Sync` in Rust, requiring zero synchronization for the FFI query path. This matches the MVP scan model (full rescan replaces the tree) and keeps the implementation simple and correct.

### 3. In-memory only at MVP — no SQLite persistence

**Decision:** The tree lives entirely in Rust process memory for the duration of its `TreeHandle` lifetime. No SQLite persistence, no on-disk index. The tree is discarded when the handle is destroyed (app close or rescan).

**Reason:** PRD §10 raises SQLite but explicitly defers schema design. PRD §22 Phase 9 places incremental indexing (with SQLite) as a later phase. Designing a schema now would be premature — the tree's query patterns aren't fully exercised until the treemap (Phase 4) and search (Phase 5) are built. In-memory is simpler, faster (no I/O), and sufficient for MVP's "scan → view → close" workflow.

### 4. ScanEvent types relocated to `common` crate

**Decision:** `ScanEvent`, `ScanSummary`, `ScanProgress`, `FileMetadata`, `DirMetadata`, and `FileAttributeFlags` are **moved from `scanner` to `common`**. `scanner` re-exports them for backward compatibility. `storage-tree` depends on `common` only.

**Reason:** ADR-003 states that same-layer crates (like `scanner` and `storage-tree`) should not depend on each other "unless a real data dependency exists at that time." The dependency here is on shared *types*, not on scanner *logic* — exactly what `common` is for. Moving the types preserves the strict layered dependency graph (`common` ← `scanner`/`storage-tree` ← `ffi`) and keeps both crates independently testable.

### 5. Two-handle lifecycle (ScanResultHandle + TreeHandle)

**Decision:** `scan_drive` returns a `ScanResultHandle` containing both the summary and the event stream. A separate `tree_create(ScanResultHandle)` call consumes (moves) the events and returns a `TreeHandle`. C# holds both handles independently.

**Reason:** Scan and tree-build are logically distinct operations with different performance characteristics and failure modes. Keeping them separate lets C# show distinct "scanning…" and "building tree…" progress states, enables independent unit testing of tree-build without running a real scan, and allows the events to be moved (not copied) into the tree — zero memory duplication. The `ScanResultHandle` retains the summary even after events are moved, so summary queries remain valid.

## Consequences

### Positive

- Tree lookup is O(1) by node id, with cache-friendly memory layout suitable for interactive treemap drill-down.
- Immutability guarantees thread-safe concurrent queries from C# without any locking.
- No premature schema design — SQLite persistence can be added in Phase 9 informed by real query patterns from Phases 4-5.
- Clean crate dependency graph with no circular dependencies, consistent with ADR-003.
- Two-handle lifecycle enables progress distinction and independent testability.

### Negative

- File-specific fields (extension, modified, created, attributes) waste ~40 bytes per directory node — accepted at 1M scale (~40 MB overhead) for the simplicity of avoiding an enum-based node type with different layouts.
- No persistence means a scan must be repeated if the app restarts — accepted for MVP, addressed in Phase 9.
- Type relocation from `scanner` to `common` is a mechanical refactor that touches several files and requires updating all import paths — one-time cost, straightforward.

## Follow-up

- SQLite persistence and incremental tree updates: Phase 9 (Incremental Indexing).
- Name string interning/dedup optimization: measure memory usage on real scans first, optimize only if profiling shows pressure.
- `top_files_by_size` implementation strategy (pre-computation vs. on-demand traversal): finalized during implementation sub-phase, measured not guessed.
