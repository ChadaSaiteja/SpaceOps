# ADR-008: Filesystem Scanner — Key Design Decisions

## Status

Accepted

## Context

Phase 2 (Filesystem Scanner) design surfaced four product-level decisions that affect scan accuracy and correctness in ways not implied by prior architecture ADRs: how reparse points are traversed, what "file size" means, how hard links are counted, and whether the `scanner` crate owns the in-memory tree or only streams events.

## Decisions

### 1. Reparse point traversal

**Decision:** Junctions, symbolic links, and mount points are **not followed**. They are surfaced as a distinct leaf node (labeled as a link/junction) with their own small metadata footprint, but their target's contents are not traversed or added to the parent's size.

**Reason:** Matches Windows Explorer and most disk-usage tools' behavior, eliminates traversal cycles entirely (no visited-set bookkeeping needed), and avoids double-counting size when multiple links point at overlapping data.

### 2. File size semantics

**Decision:** Sizes are computed as **on-disk (allocated) size** (`GetCompressedFileSize` / allocation-size equivalent), not logical file length.

**Reason:** Reflects actual disk space consumed, which is the product's core promise ("how much space is actually used"). Logical size would overstate usage for sparse and NTFS-compressed files, undermining the accuracy of the tool's central value proposition.

### 3. Hard link accounting

**Decision:** Hard-linked files are **counted at every path they appear under** (no file-index/inode-based deduplication across the scan).

**Reason:** Simpler, avoids a global dedup-tracking structure across the entire scan (memory/complexity cost), and matches the behavior of most comparable tools. Acknowledged tradeoff: total size may overcount actual unique disk usage when hard links are present. This is an accepted MVP simplification, not a correctness bug to be silently fixed — revisit only if user feedback identifies it as a real-world problem (hard links are relatively rare outside specific developer/system scenarios).

### 4. Scanner output shape

**Decision:** The `scanner` crate **streams flat entry/progress events** (`ScanEvent`, `ScanProgress`) rather than building and owning the complete in-memory tree itself. A separate Phase 3 (Storage Tree) module is responsible for assembling and owning the tree structure from that stream.

**Reason:** Keeps `scanner`'s responsibility strictly to "traverse and measure" per its ADR-003 layering, and keeps it testable without needing a tree-assembly dependency. Tree construction (parent/child relationships, in-memory representation choices) is a distinct concern belonging to Phase 3.

## Consequences

### Positive

- No cycle-detection bookkeeping needed for reparse points.
- Size figures reflect real disk usage, including sparse/compressed files.
- `scanner` stays a narrow, independently testable crate producing a stream, not a stateful tree owner.

### Negative

- Hard-linked data may be overcounted in total size figures — accepted tradeoff, documented here so it isn't mistaken for a bug later.
- Junction/symlink targets are not included in the size of the directory containing the link — users must navigate into the link's actual target path (if accessible) to see that data's own size, consistent with Explorer's behavior.

## Follow-up

- If user feedback indicates hard-link overcounting is a real problem (e.g., for developer workflows with heavy hard-link use), revisit with a file-index-tracking design as a scoped enhancement, not a default MVP requirement.
- Full `ScanEvent`/`ScanProgress` schema finalized in the Filesystem Scanner design doc (`docs/components/01-filesystem-scanner-design.md`).
