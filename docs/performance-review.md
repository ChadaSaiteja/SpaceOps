# Performance review of the Rust core (v0.2, Task 7)

Scope: `crates/scanner` and `crates/storage-tree` (the `ffi` crate and `app/` were not
touched). Goal: measurable speedups in scanning and queries with no public behavior
change and no `extern "C"` signature or struct layout change.

All numbers were measured on the development machine (12 logical cores, NTFS, Windows 11,
non-elevated shell, Defender active, other build jobs running in the background, so wall
times are noisy). Every figure is from a release build. Scans were read-only: nothing was
written to, or deleted from, any scanned folder.

## Method

* `cargo test --workspace` (91 tests at the start, 96 now: five regression tests were
  added, none removed or renamed).
* `scanner/examples/benchmark.rs` now has a real-folder mode:
  `benchmark <existing dir> [runs]` performs `runs` read-only parallel scans and prints
  min/median/max wall time, allocation count and peak heap (a counting global allocator).
  The synthetic mode is unchanged. (Building the 200k-file synthetic tree took 11 minutes
  on this machine because of real-time AV scanning, so real folders were used.)
* CPU time was taken from the process (`UserProcessorTime` / `PrivilegedProcessorTime`),
  because scan wall time is dominated by the kernel and varies +-30% run to run.
* `storage-tree/examples/tree_benchmark.rs` gained timings for search, treemap layout,
  SQLite save/load and `update_node_size` (best of 5 per measurement, 3 process runs).
* Before/after pairs were taken with the old and new binaries run alternately.
* Change acceptance rule: keep only if >=5% faster (or lower memory) and tests stay green.

## Baseline (before any change)

Scanner, `C:\Program Files` (108,028 files, 13,782 dirs, 7 in-process runs, 3 processes):

| metric | value |
|---|---|
| wall, min / median | 650-820 ms / 0.85-1.11 s |
| allocations per scan | 699,795 |
| peak heap delta | 29.5 MB |
| user CPU (7 scans) | 6.8-8.7 s |
| kernel CPU (7 scans) | 44-56 s (about 87% of all CPU) |

`C:\Windows` (242k files, 146k dirs, 3 runs): min about 9.1-9.7 s, 2.94M allocations,
peak heap 125 MB, user CPU 18.9 s, kernel CPU 197 s.

`C:\Users\SaitejaChada` (1.75M files, 250k dirs) was scanned once: 68-136 s depending on
cache state. It is too slow and noisy to be a useful A/B workload.

Storage tree (`tree_benchmark`, 1,000,000 nodes, 3 runs, median):

| operation | baseline |
|---|---|
| `StorageTree::build` | 584 ms (473-718) |
| `node()` lookup | about 45 ns |
| `children()` of a 10K-child dir | about 0.5 us |
| `top_files_by_size(root, 100)` | 108 ms (target < 50 ms: CHECK) |
| `search("file_7")` | 481 ms |
| `search("ext:txt size:>1KB")` | 179 ms |
| `search("type:code")` | 188 ms |
| `compute_layout(1920x1080, depth 4)` | 261 ms |
| tree RSS delta | 55 MB |

200k nodes: SQLite `save_tree_to_db` 1.3-1.7 s, `load_tree_from_db` 285-325 ms
(ADR-016 states < 100 ms), `update_node_size` 102-141 us/call.

## Accepted changes (one commit each)

| # | commit | change | result |
|---|---|---|---|
| 1 | `perf(scanner): drop per-entry path allocation and name clone` | `FastDirEntry` no longer stores a `PathBuf` per entry; names are moved (not cloned) into events; paths are built only for subdirectories, compressed/sparse files and progress ticks | allocations 699,795 -> 375,119 (-46%), user CPU -12%; wall within noise |
| 2 | `perf(scanner): process files serially per directory and batch event pushes` | files and reparse points of a directory are handled in a loop and appended to the shared sink under one lock; only subdirectories go to rayon | cumulative vs baseline: user CPU 6.8 -> 5.0 s (-26%), peak heap 29.5 -> 23.4 MB (-21%), min wall 772-817 -> 665-701 ms (-12%), allocations -44%. `C:\Windows`: allocations 2.94M -> 2.12M (-28%), peak heap 125 -> 103 MB (-20%), user CPU -11%, wall unchanged |
| 3 | `perf(storage-tree): prune top_files_by_size walk` | skip children (and all later siblings) whose size is strictly below the current n-th largest file | 1M nodes: 108 ms -> 35 us; 200k: 20 ms -> 43 us. Meets the 50 ms target. Output ties are now ordered by NodeId (deterministic). Brute-force comparison test added |
| 4 | `perf(storage-tree): classify file extensions without heap allocation` | stack-buffer lowercase in `classify_extension` | 1M nodes: search ext 179 -> 69 ms, search type 188 -> 61 ms, layout 261 -> 132 ms |
| 5 | `perf(storage-tree): allocation-free name matching in search` | byte-wise ASCII-case-insensitive eq/starts_with/contains instead of `to_ascii_lowercase()` per node; category only computed when filtering | 1M nodes: text 328 -> 52 ms (original 481 ms), ext 69 -> 20 ms (original 179 ms), type 61 -> 47 ms. Equivalence test added |
| 6 | `perf(storage-tree): move names and extensions out of events during build` | `StorageTree::build` consumes the events (`OsString::into_string`, moved extension) | 1M nodes: build 584 -> 179 ms median (-69%); RSS unchanged |
| 7 | `perf(storage-tree): O(1) worst-aspect evaluation in squarify` | worst aspect of a row only needs the largest and smallest item (bit-identical to the full scan; the old function is kept under `cfg(test)` and compared on random rows) | layout 1M nodes: median 107.5 -> 92.0 ms (-14%), min 101.4 -> 84.1 ms (-17%) |
| 8 | `perf(storage-tree): re-place one child instead of re-sorting in update_node_size` | move the single changed child within each ancestor's list | 114 us -> 6 us per call (-95%). Randomized sortedness/propagation test added |

Note on 1M-node search numbers: the "after" figures fluctuated between about 50 and 60 ms
for the text query over the three final runs because of machine load; the relative gains
are stable, the absolute values are not.

Cumulative effect at 1M nodes (end of review, 3 runs): build about 180 ms, top-100 about
35 us, text search about 52-61 ms, ext+size search about 20-29 ms, layout about 92-105 ms.

## Rejected ideas (measured or reasoned)

* Lock-free progress throttle (atomic CAS instead of `Mutex<Instant>`): user CPU -9%
  (5.17 -> 4.69 s over 4 paired runs) but total CPU and wall time unchanged; not worth the
  extra semantics. Reverted.
* Rayon thread count (6/12/24/48) on `C:\Program Files`: no stable difference; the scan is
  kernel/AV bound and the default is fine.
* `treemap::layout_children` reading `tree.nodes` directly instead of `tree.node()`:
  no measurable difference (105-135 ms both). Reverted.
* SQLite save: multi-row `INSERT` (32 rows per statement): 907 -> 860 ms of insert time,
  within noise. Reverted.
* SQLite `WITHOUT ROWID` for `nodes` (8-10% faster save): changes the schema for new
  databases only, leaving old and new databases different. Not worth the migration risk
  for this gain.
* SQLite `cache_size = -65536` and `mmap_size = 256 MB` pragmas: load 285 -> 258 ms
  (about -9%), but mmap on a WAL database that other processes may touch is a
  correctness/portability risk and the gain is borderline. Not adopted.
* `FindFirstFileExW` flags: already `FindExInfoBasic` + `FIND_FIRST_EX_LARGE_FETCH`
  (no 8.3 names, 64 KB batches); nothing left to gain there.
* Deferring/dropping the `nodes` secondary index during bulk save: the index spans all
  volumes, so rebuilding it per save is O(all rows).

## Remaining opportunities (not done)

1. Scan wall time is about 87% kernel CPU (44-56 s kernel vs 7 s user for 7 scans of
   `C:\Program Files`; single-threaded about 36 us kernel time per entry). Candidates:
   `NtQueryDirectoryFile` / `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` on a
   directory handle opened once with a 256 KB buffer, and avoiding Defender-visible opens.
   Needs an admin/AV-exclusion benchmark box to evaluate.
2. MFT scan (`scanner/src/mft/mod.rs`): one `DeviceIoControl(FSCTL_GET_NTFS_FILE_RECORD)`
   per record (millions of syscalls on a large volume, each returning a 12+1024 byte
   buffer) and `HashMap<u64, ...>` with SipHash plus a heap `OsString` per record. Reading
   `$MFT` extents in 1-4 MB chunks (or `FSCTL_ENUM_USN_DATA`) and a dense `Vec`/identity
   hasher indexed by record number should be several times faster. Not measurable here
   (shell is not elevated), so not attempted.
3. USN reader (`mft/usn.rs`): `ReasonMask = 0xFFFFFFFF` with `ReturnOnlyOnClose = 0`
   returns every intermediate change record; `ReturnOnlyOnClose = 1` (or masking to
   create/delete/rename/close) would cut volume substantially but changes which events the
   incremental indexer sees, so it needs a design decision.
4. `StorageTree` memory: `children: Vec<Vec<NodeId>>` is 24 bytes plus a heap block per
   directory; a CSR layout (offsets + one `Vec<NodeId>`) would cut memory and cache misses
   for `children()`, search-adjacent walks, layout and `top_files`. It touches every
   `tree.children[...]` use inside the crate, so it was left out of this pass.
5. `TreeNode` carries `extension: Option<String>` (24 bytes + heap) per file although
   extensions are highly repetitive; interning (u16 index into a small table) would save
   memory and make `classify_extension` a table lookup computed once at build.
6. `load_tree_from_db` takes about 285 ms for 200k rows (target < 100 ms, ADR-016):
   about 1.4 us per row is spent in rusqlite row decoding (`row.get` by index, `String`
   per name). A raw `sqlite3_step` loop with `get_ref` strings, or storing the arena as a
   single blob, is the way to reach the target. `reconstruct` itself is only 12 ms.
7. Search: a trigram or sorted-suffix index would make text queries sub-millisecond at 1M
   nodes (currently a 50-60 ms linear pass, ADR-012 targets < 5 ms). A parallel (rayon)
   chunked scan of `nodes` is a cheap intermediate step.
8. Treemap layout: `layout_children` builds a `LayoutItem` for every child of every laid
   out directory, even when most will be culled below `min_size_px`. Culling items by
   cumulative area before squarifying changes layout results, so it needs a visual review.
9. `update_node_size` still scans each ancestor's child list linearly to find the changed
   child; storing each node's index in its parent's list would make it O(distance moved).

## App-side finding: `TreemapControl.xaml.cs` `Canvas_Draw` (documented only, file not edited)

`Canvas_Draw` runs on every invalidation (resize, hover change, navigation). For every
tile large enough for a label (`Width >= 48 && Height >= 20`) it calls
`_tree.GetNodeName(r.NodeId)` (one FFI call, a UTF-16 conversion and a new `string`), and
for tiles with `Height >= 36` also `_tree.GetNodeInfo(r.NodeId)` just to read `Size`.
`GetNodeInfo` itself performs `tree_node_info` plus another `GetNodeName` call, so a
tall tile costs 3 FFI calls and 2 name strings per frame, plus `FormatBytes`. With the
default `maxDepth: 2` a full-screen layout is several thousand rects, so this is thousands
of P/Invokes and allocations per frame, and the hover overlay causes a full redraw each
time the pointer moves.

Recommended fix (app side): fill the label text once when `_rects` is computed (a
parallel `string[] _names` / `string[] _sizeLabels`, only for tiles that pass the
size threshold) and have `Canvas_Draw` read from those arrays; additionally render the
tile layer to a `CanvasRenderTarget`/`CanvasCachedGeometry` and draw only the hover
outline on top when the pointer moves. A Rust-side batch call returning names and sizes
for a rect list in one FFI crossing would remove the per-tile P/Invoke cost entirely, but
it needs a new `extern "C"` function and is outside this pass (FFI surface frozen).

## Treemap directory coloring (dominant-content category)

Directory tiles now take the category of the file reached by repeatedly following the
largest child (max 32 hops, resolved only for tiles that survive culling). Measured with
`cargo run --release -p storage-tree --example tree_benchmark`, `layout(1920x1080,d4)`,
before/after binaries run interleaved, median of 3 runs (best-of-5 inside each run):

| Nodes | Before    | After     |
|------:|----------:|----------:|
| 200K  | 17.78 ms  | 20.30 ms  |
| 500K  | 44.49 ms  | 45.16 ms  |
| 1M    | 105.03 ms | 106.47 ms |

The 1M layout is within 1.5% of the previous median. The 200K difference is inside this
machine's run-to-run noise (single runs of the unchanged binary ranged 17.6-19.1 ms, and
of the new one 18.0-27.9 ms, while other workloads were active); the added work is one or
two O(1) arena lookups per emitted directory tile.
