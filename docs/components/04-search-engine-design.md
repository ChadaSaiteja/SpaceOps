# Search & Index Engine — Component Design

Status: **Approved design for Phase 5.**

Governed by: ADR-001 (FFI), ADR-003 (module boundaries), ADR-007 (threading), ADR-009 (StorageTree arena), ADR-010 (Treemap navigation), ADR-012 (Search Engine Architecture).

## 1. Problem Statement

Given an active in-memory `StorageTree` containing up to millions of filesystem nodes, users need to find specific files, folders, extensions, or storage hogs with sub-10ms keystroke latency, rich filtering (by name, extension, size, and category), and immediate visual drill-down into the Treemap.

## 2. Requirements

- **Sub-5ms Query Latency**: Searching across 1,000,000 in-memory nodes must complete in $< 5\text{ ms}$ on a modern CPU without freezing the UI thread.
- **Zero-Copy In-Memory Search**: Search directly over the contiguous `StorageTree` arena without duplicating path or name strings into a secondary memory-heavy index.
- **Rich Query Syntax**:
  - Plain text: Case-insensitive substring and prefix matching on file/directory names.
  - Extension filter: `ext:vhdx`, `ext:iso`, `ext:log`.
  - Size filter: `size:>1GB`, `size:<10MB`, `size:50MB..500MB` (supporting B, KB, MB, GB, TB).
  - Category filter: `type:video`, `type:dev`, `type:archive`, `type:system`.
  - Kind filter: `kind:file`, `kind:dir`.
- **Space-Aware Relevance Ranking**:
  - Score components:
    1. Match quality: Exact name match (1000) > Prefix match (500) > Substring match (100).
    2. Size weight: Larger items ranked higher within the same match tier (essential for disk-space investigation).
    3. Depth penalty: Top-level items slightly favored over deeply buried files.
- **Bounded Top-K Results**: Return top $N$ items (default 100) to keep FFI transmission and UI rendering lightweight.
- **Bidirectional UI Integration**:
  - Search box with debounced real-time suggestions.
  - Selecting a search result highlights the item and automatically drills down to its location in the Treemap and breadcrumbs.

## 3. Architecture & Query Pipeline

```text
User Types Query in WinUI SearchBox (debounced 150ms)
                     │
                     ▼
           SearchService.SearchAsync()
                     │ (P/Invoke)
                     ▼
         tree_search(tree_handle, query, max_results)
                     │
                     ▼
             Parse Query Tokens
   ┌─────────────────┼──────────────────┐
   ▼                 ▼                  ▼
Text Matcher    Size Range Matcher   Category/Ext Filter
   │                 │                  │
   └─────────────────┼──────────────────┘
                     │
                     ▼
          Linear / Parallel Scan of Arena
          (Rayon par_iter over Vec<TreeNode>)
                     │
                     ▼
        Top-K Heap Filter (BinaryHeap)
                     │
                     ▼
     Sorted SearchResultFfi Array Returned (< 3ms)
                     │
                     ▼
  WinUI Results Flyout -> User Click -> Treemap Drill-down
```

## 4. Query Grammar & Syntax Model

```text
Query       := Token (WS+ Token)*
Token       := FilterToken | TextToken
FilterToken := Prefix ":" Value
Prefix      := "ext" | "size" | "type" | "kind"
TextToken   := String (case-insensitive substring match)

SizeValue   := (">" | "<" | ">=" | "<=") Number Unit
             | Number Unit ".." Number Unit
Unit        := "B" | "KB" | "MB" | "GB" | "TB"
```

Examples:
- `docker ext:vhdx` — Files containing "docker" with extension `.vhdx`
- `size:>1GB type:video` — All video files larger than 1 GB
- `node_modules kind:dir` — Find all `node_modules` folders
- `*.iso size:>500MB` — ISO files larger than 500 MB

## 5. Performance Engineering

1. **Arena Direct Traversal**:
   Since `StorageTree` (Phase 3) is a contiguous `Vec<TreeNode>` in RAM (55 MB for 1M nodes), traversing it in parallel via `rayon` achieves full cache-line streaming. 1,000,000 nodes are evaluated in ~2–4 ms with zero allocation per candidate.
2. **Top-K Bounded Heap**:
   Using a min-heap of size $K$ (`BinaryHeap<Reverse<RankedResult>>`) ensures we only retain the top 50–100 items without sorting the entire million-element candidate list.

## 6. FFI & C-ABI Specification

```rust
#[repr(C)]
pub struct SearchResultFfi {
    pub node_id: u64,
    pub name: FfiString,
    pub path: FfiString,
    pub allocated_size: u64,
    pub is_directory: u8,
    pub category: u8,
    pub score: u32,
}
```
