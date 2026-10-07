# ADR-016: Incremental Indexing, SQLite Persistence & USN Journal Synchronization

## Status

Accepted

## Context

Previous phases established the foundational scanning (`scanner`), arena storage tree (`storage-tree`), treemap visualization, search engine, cleanup engine, application manager, and developer storage engine. However, every new launch of the application currently requires initiating a full directory crawl to construct the in-memory tree, taking 30–90 seconds for a full drive or several seconds for deep project hierarchies.

PRD §10 ("Database / Index") and PRD §22 (Phase 9 — "Incremental Indexing") specify:
1. SQLite-backed persistent index for scanned volumes and directories.
2. Fast launch: hydrating an in-memory `StorageTree` directly from the local database in **< 100 ms** without crawling physical disks.
3. Change detection & incremental updating:
   - On NTFS volumes with elevation: querying the volume's Update Sequence Number (`USN`) change journal (`FSCTL_READ_USN_JOURNAL`) to process filesystem deltas in **< 50 ms**.
   - On non-elevated or non-NTFS volumes: running differential scans comparing directory modification timestamps against indexed timestamps.
4. Dynamic in-memory tree patching: propagating size deltas ($\Delta_{\text{size}}$) upwards through ancestor chains without rebuilding the tree from scratch.

This ADR establishes the architectural decisions, database schema, synchronization lifecycle, and safety guarantees for Phase 9.

---

## Decisions

### 1. Embedded SQLite with Bundled Engine & WAL Mode

**Decision:** The application embeds SQLite directly via `rusqlite` (with `bundled` C engine), storing the index at `%LOCALAPPDATA%\StorageIntelligence\storage_index.db` (or a caller-specified path).
The database connection initializes with optimal performance pragmas:
```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -64000; -- 64 MB cache
```

**Reason:**
WAL mode allows non-blocking concurrent reads during background batch commits, while bundled SQLite eliminates runtime DLL version conflicts across diverse Windows installations.

---

### 2. Relational Schema for Volume & Node Hierarchy

**Decision:** The database defines a normalized two-table schema:

```sql
CREATE TABLE IF NOT EXISTS volumes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    drive_or_path TEXT UNIQUE NOT NULL,
    volume_guid TEXT,
    fs_type TEXT,
    cluster_size INTEGER NOT NULL,
    last_usn INTEGER NOT NULL DEFAULT 0,
    usn_journal_id INTEGER NOT NULL DEFAULT 0,
    last_scan_time INTEGER NOT NULL,
    total_bytes INTEGER NOT NULL DEFAULT 0,
    free_bytes INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS nodes (
    id INTEGER PRIMARY KEY,           -- Matches NodeId in StorageTree arena
    volume_id INTEGER NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    parent_id INTEGER,               -- Nullable parent NodeId
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    file_count INTEGER NOT NULL,
    dir_count INTEGER NOT NULL,
    kind INTEGER NOT NULL,           -- 0 = Directory, 1 = File, 2 = ReparsePoint
    extension TEXT,
    modified INTEGER,
    created INTEGER,
    attributes INTEGER NOT NULL DEFAULT 0,
    usn INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_nodes_volume_path ON nodes(volume_id, path);
CREATE INDEX IF NOT EXISTS idx_nodes_volume_parent ON nodes(volume_id, parent_id);
```

**Reason:**
This schema enables instant subtree retrieval by parent ID, rapid path-based lookups, and clean atomic cascade deletion when a volume or root is rescanned.

---

### 3. Dual-Tier Change Detection: USN Journal + Timestamp Differential

**Decision:** Change detection operates under a dual-tier strategy:

- **Tier 1 (NTFS USN Journal):**
  When scanning an NTFS drive root (e.g., `C:\`) with available elevation, the engine opens `\\.\X:` and issues `FSCTL_QUERY_USN_JOURNAL`. If the journal ID matches `usn_journal_id` stored in the database and `last_usn` is within valid journal bounds, it reads subsequent change records via `FSCTL_READ_USN_JOURNAL`:
  - `USN_REASON_FILE_CREATE`: creates a new node in tree and database.
  - `USN_REASON_FILE_DELETE`: marks or removes the node from tree and database.
  - `USN_REASON_DATA_OVERWRITE | DATA_EXTEND | DATA_TRUNCATION`: fetches updated file allocation size, updates the node, and bubbles size differences up ancestor paths.
  - `USN_REASON_RENAME_NEW_NAME`: updates node name and reconstructed path.
- **Tier 2 (Timestamp Differential Fallback):**
  When USN Journal is unavailable (non-admin, subfolder target, FAT32/exFAT/ReFS/network shares, or journal ID mismatch/journal wrap):
  The engine traverses directory nodes whose on-disk `LastWriteTime` is greater than the indexed `modified` timestamp. Only modified subdirectories are re-enumerated, avoiding $O(N)$ full disk traversal.

**Reason:**
Guarantees near-instantaneous delta syncing ($< 50\text{ ms}$) on standard NTFS drives while maintaining robust incremental capabilities on all arbitrary paths and privilege levels.

---

### 4. Fast Subtree Propagation & In-Memory Patching

**Decision:** During incremental updates, existing nodes in the `StorageTree` arena are patched in place rather than re-allocating a new arena:
1. When a node's size changes: $\Delta_{\text{size}} = \text{size}_{\text{new}} - \text{size}_{\text{old}}$, and $\Delta_{\text{files}} = \text{count}_{\text{new}} - \text{count}_{\text{old}}$.
2. Starting from the parent node, the difference is added to every ancestor's `.size` and `.file_count` until reaching the root (`node.parent == None`).
3. For a maximum path depth of 32–64, propagation completes in $< 1\text{ µs}$ per modified file.

**Reason:**
Preserves active UI state, treemap drill-down context, and selected item pointers without full teardown and rebuilding.

---

### 5. Transaction Chunking & Atomic Persistence

**Decision:** When writing tree nodes to SQLite:
- Nodes are inserted in chunks of 5,000 to 10,000 records inside an explicit transaction (`BEGIN TRANSACTION` ... `COMMIT`).
- Wal checkpointing is managed automatically (`PRAGMA wal_autocheckpoint = 1000`).
- Total persistence time for 1,000,000 nodes is constrained to $< 1.5\text{ s}$.

**Reason:**
Single-transaction batching eliminates disk flush overhead per node while keeping memory overhead during commits bounded.

---

## Consequences

### Positive
- **Instant App Launch:** Hydrating from local SQLite enables rendering a previously scanned drive in **< 100 ms**.
- **Instant Rescans:** Rescanning a drive via USN Journal or differential timestamps takes **< 50 ms** instead of 30–90 seconds.
- **Persistence Across Sessions:** Historical scan records, total sizes, and previous states survive application restarts.
- **ACID Integrity:** SQLite WAL transactions prevent corrupted state if the application is closed or restarted during a sync.

### Negative / Trade-offs
- Adds `rusqlite` (bundled C engine) to the build dependencies (~1.2 MB disk space, compiles in ~18s once).
- Database file consumes $\approx 50\text{–}70\text{ MB}$ per 1,000,000 indexed files on disk in `%LOCALAPPDATA%`.
