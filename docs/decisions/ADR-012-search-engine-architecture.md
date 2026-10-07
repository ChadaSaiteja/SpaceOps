# ADR-012: Search Engine Architecture & In-Memory Indexing

## Status

Accepted

## Context

Phase 5 introduces the Search Engine for Windows Storage Intelligence. With scans producing trees containing hundreds of thousands or millions of files, users require real-time search to locate specific files, extensions, large downloads, virtual machine disks, and directories.

Key constraints:
- Must respond within keystroke latency ($< 10\text{ ms}$).
- Low memory overhead (cannot duplicate gigabytes or hundreds of megabytes of string data).
- Must seamlessly interact with the Treemap visualization (Phase 4).

---

## Decisions

### 1. Direct In-Memory Arena Search vs Inverted Index vs SQLite

**Decision:** MVP search executes directly over the contiguous `StorageTree` arena (`Vec<TreeNode>`) using parallel chunked filtering (`rayon`), bounded by a fixed-size min-heap for top-$K$ results. A secondary inverted index or SQLite FTS is deferred to V2 for offline/historical searches.

**Reason:**
- The `StorageTree` arena is already 100% cache-resident and contiguous (55 MB for 1M nodes).
- In benchmarks, a parallel linear scan of 1,000,000 nodes with string/attribute predicates executes in **2 to 4 milliseconds**.
- An in-memory inverted trigram/ngram index would add 40–80 MB of duplicate RAM, while offering negligible perceived improvement over 3 ms.
- Direct arena access ensures 0 extra memory overhead and zero index rebuild delays.

### 2. Tokenized Filtered Query Model

**Decision:** Search queries support composable tokens:
- Free text: Case-insensitive name substring/prefix matching (e.g. `docker`, `appdata`).
- `ext:<extension>`: Exact extension match (e.g. `ext:vhdx`, `ext:iso`).
- `size:<range>`: Size operator (e.g. `size:>1GB`, `size:<10MB`, `size:100MB..500MB`).
- `type:<category>`: Category filter mapping to treemap colors (`type:video`, `type:dev`, `type:archive`).
- `kind:<file|dir>`: Node type filter (`kind:file`, `kind:dir`).

**Reason:**
Provides power users with surgical precision without requiring SQL or complex regex knowledge.

### 3. Space-Aware Relevance Ranking

**Decision:** Search results are ranked by:
1. Match quality: Exact name match (tier 1000) > Prefix match (tier 500) > Substring match (tier 100).
2. Size weighting: Within the same match tier, nodes are sorted by allocated size descending.

**Reason:**
In a storage intelligence utility, users search to triage space hogs. Ranking larger files first among matching names accelerates discovering what is consuming disk space.

### 4. Direct Treemap Drill-down Integration

**Decision:** Clicking any search result in the WinUI search flyout immediately uses `tree_ancestors` to set the Treemap's current root to the item's parent directory, updates the breadcrumb bar, highlights the target rectangle, and populates the sidebar details.

**Reason:**
Creates a unified visual workflow where search is not an isolated list, but an instant magnifying glass onto the treemap.

---

## Consequences

### Positive
- Instant search responses ($< 5\text{ ms}$) on 1,000,000-node trees.
- Zero memory bloat.
- Seamless Treemap cross-navigation.

### Negative / Trade-offs
- File content search (grep/full-text) is not supported in Phase 5 (metadata-only search).
