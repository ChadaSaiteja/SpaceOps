# Filesystem Scanner — Component Design

Status: **Approved design. Multi-Tier Hybrid Architecture (Revision 2).**

Governed by: ADR-001 (FFI), ADR-003 (module boundaries), ADR-004 (errors), ADR-007 (threading), ADR-008 (scanner-specific decisions), ADR-011 (multi-tier hybrid scanner & USN persistence).

## 1. Problem

Given a root path (drive or directory), traverse the filesystem and produce accurate size aggregates and metadata at industry-leading speed, tolerating Windows-specific conditions (permissions, junctions, locked files) without aborting, while reporting progress, honoring cancellation, persisting snapshots, and enabling sub-50ms incremental updates.

## 2. Requirements

- Multi-tier scanning: Direct NTFS MFT for whole-drive admin scans (~1-2s), parallel kernel-buffered Win32 (`FindFirstFileExW` + `FIND_FIRST_EX_LARGE_FETCH`) for user-mode / subfolder scans, and shallow depth-bounded scanning (<100ms) for instant UI response (ADR-011).
- Persistence & USN Journal incremental indexing: Persist scans to SQLite with volume high USN; rescan via `FSCTL_READ_USN_JOURNAL` in milliseconds (ADR-011).
- Resilient fallback: Mid-scan MFT failures cleanly discard partial state and restart with Win32 under a fresh token; never merge disjoint ID spaces (ADR-011).
- On-disk (allocated) size semantics, not logical size (ADR-008 #2).
- Do not follow reparse points/junctions/symlinks (ADR-008 #1).
- Hard links counted at every path (ADR-008 #3), no dedup tracking.
- Tolerate permission-denied, locked files, broken reparse points as per-item issues (ADR-004 tier 1), not scan failures.
- Stream flat events compatible with Storage Tree arena (ADR-008 #4).

## 3. Architecture & Engine Selection

```text
                        scan_volume_resilient(options)
                                      │
            ┌─────────────────────────┴─────────────────────────┐
            ▼                                                   ▼
[Admin & Full NTFS Volume]                           [Standard User or Subfolder]
            │                                                   │
     Tier 1: MFT Engine                                 Tier 2: Win32 Engine
   (Direct \\.\X: $MFT Read)                         (FindFirstFileExW + LARGE_FETCH)
            │                                                   │
     Success: 1-2s ────┐                                        │
            │          │                                        │
     Mid-scan error    │                                        │
            │          │                                        │
            ▼          │                                        ▼
   [Clean Discard &    │                              [Progressive Streaming]
       Restart] ───────┼────────────────────────────► (Shallow <100ms -> Deep walk)
                       │                                        │
                       ▼                                        ▼
               [Persist to SQLite] <────────────────────────────┘
               [Record High USN]
                       │
                       ▼
            [Next Launch / Rescan]
                       │
            Tier 0: USN Journal Delta
              (FSCTL_READ_USN_JOURNAL)
                    (< 50ms)
```

## 4. Rust Modules (`core/crates/scanner`)

```text
scanner/
├── lib.rs              # Public API: scan_volume_resilient(), ScanEngine, ScanOptions
├── winfs.rs            # Win32 kernel-buffered enumeration (FindFirstFileExW, LARGE_FETCH)
├── metadata.rs         # Win32 find-data extraction, cluster rounding, sparse sizing
├── traversal.rs        # Parallel work-stealing directory walk (rayon) & shallow mode
├── mft/                # Tier 1 MFT engine
│   ├── mod.rs          # MFT scan orchestrator
│   ├── volume.rs       # Volume handles (\\.\X:), FSCTL_GET_NTFS_FILE_RECORD
│   ├── record.rs       # 1024-byte record parser ($STANDARD_INFO, $FILE_NAME, $DATA)
│   └── usn.rs          # Tier 0 USN journal change reader (FSCTL_READ_USN_JOURNAL)
└── error.rs            # Error mapping and engine-specific error tiering
```

## 5. Clean Discard-and-Restart Fallback Protocol

MFT parsing allocates IDs based on physical record indices, whereas Win32 directory traversal generates monotonic sequential `NodeId`s.
- If `scan_mft` encounters a fatal block read or permission revocation mid-scan:
  1. Abort MFT task and immediately deallocate partial MFT buffers.
  2. Instantiate a fresh `CancellationToken` and clean `ScanContext`.
  3. Seamlessly restart scan using `walk_dir` (Win32 parallel traversal).
  4. Never attempt to stitch or merge partial MFT nodes into the Win32 tree.

## 6. Persistence & USN Journal Incremental Index

1. **Volume Snapshot**: Persisted into SQLite with `(volume_guid, last_usn, timestamp)`.
2. **Subsequent Rescans**:
   - Query current journal via `FSCTL_QUERY_USN_JOURNAL`.
   - Read delta records via `FSCTL_READ_USN_JOURNAL` starting from `last_usn`.
   - Patch in-memory Storage Tree and SQLite index:
     - `FILE_CREATE`: Insert node.
     - `FILE_DELETE`: Delete node, adjust parent sizes.
     - `DATA_EXTEND / TRUNCATION`: Adjust file size, propagate delta to root.
     - `RENAME`: Update node name/parent.
   - Updates complete in **< 50ms**.

## 7. 15-Case Cross-Engine Parity Test Matrix

All 3 engines must satisfy all 5 ADR-008 invariants:

| Invariant | Description | MFT Engine | Win32 Engine | Shallow Engine | Parity Validation |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **1. Reparse Points** | Junctions/symlinks are non-traversed leaves | Checked | Checked | Checked | Identical size & non-traversal |
| **2. Allocated Size** | Cluster rounding + compressed/sparse | Checked | Checked | Checked | Byte-for-byte cluster math |
| **3. Hard Links** | Counted at every path | Checked | Checked | Checked | Identical file counts |
| **4. Flat Streaming** | Emits `EnteredDirectory`, `FileFound`, `DirectoryComplete` | Checked | Checked | Checked | Identical tree reconstruction |
| **5. Error Tiering** | Inaccessible items recorded, do not abort | Checked | Checked | Checked | Identical inaccessible count |
