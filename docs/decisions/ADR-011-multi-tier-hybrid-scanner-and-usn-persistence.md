# ADR-011: Multi-Tier Hybrid Scanner, USN Journal Persistence & Clean Fallback

## Status

Accepted

## Context

Phase 2 established a baseline parallel filesystem scanner using `rayon` and Rust's `std::fs`. While functionally correct, scanning a 1,000,000-file Windows drive (`C:\`) required a full synchronous crawl before UI display (taking 30–90 seconds) and incurred significant per-file syscall overhead (`std::fs::symlink_metadata`).

Benchmarking industry-leading tools (WizTree, Everything, TreeSize, DaisyDisk) demonstrates two critical performance vectors:
1. Direct NTFS MFT (Master File Table) raw parsing yields whole-volume index construction in **1–2 seconds**.
2. Change-journal (USN Journal) incremental indexing allows rescans to process only filesystem deltas in **< 50 milliseconds**.
3. Progressive shallow-first traversal allows immediate UI rendering (< 100ms) while background crawling completes.

This ADR defines the architectural decisions governing the multi-tier scanner, incremental index persistence, fallback mechanics, and cross-engine parity testing.

---

## Decisions

### 1. Multi-Tier Hybrid Scanning Architecture

**Decision:** The application implements three scanning tiers governed by an automated supervisor (`scan_volume_resilient`):
- **Tier 1 (MFT Engine)**: Activated when scanning a whole NTFS drive with Administrator elevation. Reads `$MFT` directly from `\\.\X:` in large sequential chunks.
- **Tier 2 (Optimized Win32 Engine)**: Activated for standard user processes, non-NTFS volumes, or targeted subfolders. Uses `FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)` and extracts metadata directly from `WIN32_FIND_DATAW`, eliminating redundant `symlink_metadata` calls.
- **Tier 3 (Shallow Progressive Mode)**: Immediate enumeration of depth 1 (< 100ms) streamed to the UI, enabling instant visual feedback while background threads populate child sizes with interactive priority boosting.

**Reason:**
Provides market-dominating speed (~2s) when elevated on NTFS, while preserving 100% functionality without elevation or on non-NTFS drives.

### 2. SQLite Persistence and USN Journal Incremental Indexing

**Decision:** Completed scans are persisted to a local SQLite database along with the volume's high Update Sequence Number (`USN`). On subsequent launches or rescan requests, the scanner calls `FSCTL_READ_USN_JOURNAL` starting from `last_usn` and patches the database and in-memory tree incrementally.

**Reason:**
A 2-second initial scan only delivers maximum value if the user does not need to repeat it. USN delta processing takes **10–50 ms**, making repeat analysis feel instantaneous.

### 3. Clean Discard-and-Restart Fallback Protocol

**Decision:** If the MFT engine fails mid-scan (e.g., at 60% due to unreadable records, device locks, or privilege revocation), the engine **must completely discard partial MFT state and restart using the parallel Win32 engine under a fresh token**. Partial MFT and Win32 trees are never stitched or merged.

**Reason:**
MFT record numbers and Win32 runtime monotonic `NodeId`s belong to completely disjoint identifier spaces. Attempting to stitch disjoint ID spaces causes corrupted hierarchies, duplicate node counts, and memory leaks. A clean restart with a fresh `CancellationToken` ensures rock-solid data integrity.

### 4. 15-Case Cross-Engine Parity Test Matrix

**Decision:** All three engines (MFT, Win32 Large-Fetch, Shallow) must pass tests asserting strict compliance with all 5 ADR-008 invariants:
1. Reparse points (junctions/symlinks) are leaves and never traversed.
2. On-disk size uses cluster allocation rounding + sparse/compressed handling.
3. Hard links are counted at every directory path.
4. Output conforms to flat streaming events (`EnteredDirectory`, `FileFound`, `DirectoryComplete`).
5. Error tiering: per-item access errors do not abort the scan.

Furthermore, cross-engine parity tests on identical directory fixtures must assert equal byte aggregates, file counts, and directory counts.

**Reason:**
Prevents divergence between engines and guarantees that switching between MFT and Win32 yields identical user-visible disk usage figures.

---

## Consequences

### Positive
- Drive scans drop from ~60s down to **1–2 seconds** (MFT) or **< 50ms** (USN incremental rescan).
- UI displays top-level cards and skeleton treemap in **< 100ms**.
- Zero redundant `GetFileAttributesExW` syscalls in the Win32 path.
- 100% safe fallback with zero ID corruption.

### Negative / Trade-offs
- Low-level MFT parsing requires managing raw NTFS structures and volume handles.
- Incremental updates require local SQLite schema management.
