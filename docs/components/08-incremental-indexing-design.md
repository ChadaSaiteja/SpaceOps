# Incremental Indexing, SQLite Persistence & USN Journal Synchronization — Component Design

Status: **Approved design for Phase 9.**

Governed by: ADR-001 (FFI), ADR-003 (module boundaries), ADR-007 (threading), ADR-009 (StorageTree arena), ADR-011 (Multi-Tier Hybrid Scanner), ADR-016 (Incremental Indexing & Persistence), PRD §10 (Database / Index), PRD §22 (Phase 9).

---

## 1. Problem Statement

Full disk scanning requires walking hundreds of thousands to millions of files, taking 30–90 seconds per cold scan. In everyday user workflows:
1. Most files on a drive do not change between application launches.
2. Cold-starting the application should not force users to wait for a full disk crawl before inspecting previous storage usage.
3. Rescans should take milliseconds, capturing only recent changes rather than crawling every unchanged folder.

The **Incremental Indexing & Persistence Engine** provides:
- **Instant Launch (< 100 ms):** Loads existing volume metadata and arena tree from local SQLite.
- **Microsecond Rescans (< 50 ms):** Syncs filesystem deltas via NTFS USN Change Journal or directory timestamp differentials.
- **Dynamic In-Memory Tree Patching:** Re-aggregates sizes along ancestor paths without rebuilding the arena from scratch.

---

## 2. System Architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│                    WinUI 3 Storage Application                  │
│       - Instant Workspace Hydration from Saved Index            │
│       - One-Click Fast Rescan / Incremental Sync                │
│       - Sync Status Indicator (Unchanged / Patched Files)       │
└────────────────┬────────────────────────────────┬───────────────┘
                 │                                │
                 ▼                                ▼
      ┌──────────────────────┐        ┌──────────────────────┐
      │     IndexService     │        │  StorageTreeService  │
      │  (Coordinator & FFI) │        │  (Interactive Tree)  │
      └──────────┬───────────┘        └───────────┬──────────┘
                 │                                │
                 ▼                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Rust FFI Layer                           │
│   index_open()          index_save_tree()    index_load_tree()  │
│   index_sync_tree()     index_get_stats()    index_close()      │
└────────────────┬────────────────────────────────┬───────────────┘
                 │                                │
                 ▼                                ▼
┌──────────────────────────────────┐ ┌────────────────────────────┐
│      Incremental Sync Engine     │ │   SQLite Persistence DB    │
│  - NTFS USN Journal Reader       │ │  - Embedded rusqlite (WAL) │
│  - Directory Timestamp Diff      │ │  - volumes & nodes tables  │
│  - Ancestor Size Bubble-Up       │ │  - 10k-batch transactions  │
└──────────────────────────────────┘ └────────────────────────────┘
```

---

## 3. Data Flow & Lifecycle

### 3.1 Initial Save Lifecycle
1. User scans a path (e.g. `C:\` or `D:\projects`).
2. An in-memory `StorageTree` is built from scanner events.
3. User or application calls `index_save_tree(db_path, tree_handle, path)`.
4. The engine opens SQLite, creates `volumes` and `nodes` tables if needed, captures current volume USN state (`usn_journal_id`, `next_usn`), and bulk-inserts all arena nodes inside a single transaction.

### 3.2 Instant Hydration Lifecycle (< 100 ms)
1. On app launch or path selection, application calls `index_load_tree(db_path, path, out_tree)`.
2. The engine verifies if an index exists for the target path.
3. If present, it queries all nodes ordered by ID, reconstructs the `StorageTree` arena directly in memory in $< 100\text{ ms}$, and returns an owned `TreeHandle`.
4. WinUI 3 treemap renders immediately without any physical disk traversal.

### 3.3 Incremental Sync Lifecycle (< 50 ms)
1. User clicks "Rescan" or selects "Sync Changes".
2. Application calls `index_sync_tree(db_path, tree_handle, path, out_report)`.
3. If elevated NTFS root:
   - Queries `FSCTL_READ_USN_JOURNAL` starting from `last_usn`.
   - Dispatches creates, deletes, and size changes to tree and database.
4. If non-admin / non-NTFS:
   - Evaluates subdirectories whose on-disk `LastWriteTime > node.modified`.
   - Traverses only changed directories, patching the arena nodes and updating ancestors.
5. Tree node sizes are re-aggregated upwards:
   $$\text{parent.size} \leftarrow \text{parent.size} + \Delta_{\text{size}}$$
6. Updates SQLite records and returns `IndexSyncReport` (`nodes_added`, `nodes_updated`, `nodes_removed`, `bytes_delta`).

---

## 4. API & FFI Contracts

```rust
#[repr(C)]
pub struct IndexStatsFfi {
    pub volume_id: u64,
    pub node_count: u64,
    pub total_size: u64,
    pub last_scan_time: u64,
    pub last_usn: u64,
}

#[repr(C)]
pub struct IndexSyncReportFfi {
    pub nodes_added: u64,
    pub nodes_updated: u64,
    pub nodes_removed: u64,
    pub bytes_delta: i64,
    pub sync_duration_ms: u64,
}
```

Exports:
- `index_save_tree(db_path: *const u16, tree: *const TreeHandle, root_path: *const u16) -> i32`
- `index_load_tree(db_path: *const u16, root_path: *const u16, out_tree: *mut *mut TreeHandle) -> i32`
- `index_sync_tree(db_path: *const u16, tree: *mut TreeHandle, root_path: *const u16, out_report: *mut IndexSyncReportFfi) -> i32`
- `index_get_stats(db_path: *const u16, root_path: *const u16, out_stats: *mut IndexStatsFfi) -> i32`
- `index_delete_volume(db_path: *const u16, root_path: *const u16) -> i32`
