# SpaceOps — Open-Source Storage Analyzer for Windows

<div align="center">

**Open-source storage intelligence for Windows.**

*Understand your storage. Clean it safely.*

[![CI](https://github.com/ChadaSaiteja/SpaceOps/actions/workflows/ci.yml/badge.svg)](https://github.com/ChadaSaiteja/SpaceOps/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%20%2F%2011-0078D6?logo=windows)](https://microsoft.com/windows)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![.NET](https://img.shields.io/badge/.NET-9.0-512BD4?logo=dotnet)](https://dotnet.microsoft.com/)
[![Tests](https://img.shields.io/badge/Tests-121%20Passed-brightgreen)](https://github.com/ChadaSaiteja/SpaceOps/actions/workflows/ci.yml)

[What is SpaceOps?](#what-is-spaceops) •
[Features](#what-does-spaceops-detect) •
[Why is my disk filling up?](#why-is-my-disk-filling-up-on-windows) •
[Comparison](#how-does-spaceops-compare-to-windirstat-treesize-and-wiztree) •
[Quick Start](#how-do-i-install-and-run-spaceops) •
[Architecture](#architecture) •
[Roadmap](#whats-planned-next) •
[FAQ](#frequently-asked-questions) •
[Website](https://chadasaiteja.github.io/SpaceOps/) •
[Contributing](#community--contributing)

</div>

---

## What is SpaceOps?

SpaceOps is a free, open-source **disk analyzer and storage cleaner for Windows 10 and 11**. It scans your drives with a high-speed Rust engine and visualizes disk usage as an interactive treemap, so you can see exactly which folders, files, and developer caches are consuming your disk space — then reclaim space safely through the Windows Recycle Bin.

**Who is it for?** Developers, technical professionals, and power users whose disks are filling up with build artifacts, container images, and package caches.

**How is it different from WinDirStat, TreeSize, or WizTree?** SpaceOps is free and MIT-licensed, detects clutter across 8 developer ecosystems (Node.js, Rust, .NET, Python, Java, Docker/WSL, Git, IDEs), and never deletes permanently — every cleanup routes through the Recycle Bin so you can undo mistakes.

**Project website:** [chadasaiteja.github.io/SpaceOps](https://chadasaiteja.github.io/SpaceOps/)

Instead of acting as a blind "disk cleaner" that arbitrarily deletes temporary files, SpaceOps provides deep visibility into your storage. It combines a high-speed **Rust scanning engine** with a 120 FPS hardware-accelerated **Direct2D squarified treemap**, deep detection for **8 developer toolchains**, and a **zero-accident safety model** that routes cleanups to the Windows Recycle Bin.

**Maintained by** [Saiteja Chada](https://github.com/ChadaSaiteja) · MIT licensed · Windows 10/11 x64 and ARM64

---

## Why is my disk filling up on Windows?

On modern Windows workstations, gigabytes of storage quietly vanish into:

- **Docker Desktop & WSL2 Virtual Disks**: Expanding `.vhdx` files that don't shrink automatically when containers or Linux packages are deleted.
- **Deep Dependency Trees**: Nested `node_modules`, pnpm virtual stores, and yarn caches across abandoned projects.
- **Incremental Build Directories**: Gigabytes of debug symbols (`.pdb`) and compilation caches in Rust `target/`, .NET `bin/` and `obj/`, and Java `.gradle/` folders.
- **Python Virtual Environments**: Redundant site-packages and wheel caches in forgotten `.venv` directories.
- **Application Leftovers**: Registry entries and orphaned AppData directories remaining after uninstallation.

Traditional disk tools treat these as plain directories or risk corrupting active projects. SpaceOps answers three fundamental questions within seconds:

> **1. What is using my storage?**  
> **2. Why is it using so much?**  
> **3. What can I safely do about it?**

---

## What does SpaceOps detect?

### ⚡ 1. High-Performance Filesystem Scanner
- **Parallel Traversal**: Multi-threaded traversal using Rayon and Win32 kernel APIs (`FindFirstFileExW`).
- **Cluster Allocation Accuracy**: Measures true physical on-disk space (aligned to 4KB filesystem clusters) rather than logical byte sizes.
- **Reparse-Point Boundary Guard**: Detects junction points, directory symlinks, and volume mounts, treating them as leaves to prevent infinite traversal loops.
- **Real-Time Throttled Streaming**: Streams live file and byte counts to the UI at 60 Hz without dispatcher lag.

### 🗺️ 2. Hardware-Accelerated Interactive Treemap
- **Direct2D / Win2D Rendering**: Smooth 60/120 FPS canvas with Mica window backdrops and dark/light mode integration.
- **Squarified Layout Algorithm**: Computes tile geometry via the Bruls-Huizing-van Wijk algorithm in Rust, maintaining balanced aspect ratios and preventing thin, illegible slivers.
- **Interactive Drill-Down**: Double-click to zoom into any subdirectory; single-click to inspect properties in the metadata pane.
- **Breadcrumb Navigation**: Instant ancestor jump bar for navigating back up deeply nested paths.

### 🛠️ 3. Developer Storage Intelligence
Identifies dormant build artifacts, virtual disks, and package caches across **8 ecosystems**:
- **Node.js**: `node_modules`, npm cache, pnpm store, yarn cache.
- **Rust**: Cargo `target/` directories, cargo registry cache.
- **.NET / C#**: `bin/`, `obj/`, global NuGet package caches.
- **Python**: `venv`, `.venv`, `__pycache__`, `.pytest_cache`, pip wheel caches.
- **Java / JVM**: `.gradle/caches`, `.m2/repository`.
- **Containers & WSL**: Docker Desktop VHDX files, WSL2 `ext4.vhdx`, buildkit storage.
- **Version Control**: Stale `.git/objects/pack` garbage.
- **IDE Caches**: VS Code workspace storage, Visual Studio `.vs/` directories.
- **Dormancy Analysis**: Flags projects untouched for $\ge 30\text{ days}$ as primary reclamation targets.

### 🛡️ 4. Zero-Accident Safe Cleanup
SpaceOps adheres to the safety rule:  
*"Analyze first. Explain second. Recommend third. Delete last."*
- **Immutable System Blacklist**: Strict kernel protections guarding `C:\Windows`, `C:\Program Files`, boot managers, and user system profiles.
- **Recycle Bin Routing**: Deletions route through Windows Shell COM (`IFileOperation`), allowing instant undo.
- **Dry-Run Mode**: Simulates byte reclamation and file counts without touching the filesystem.

### 🔍 5. Sub-5ms In-Memory Search
- Searches indexed storage trees in under 5 milliseconds.
- Supports structured syntax filters:
  - `ext:log` or `ext:iso`
  - `size:>1GB` or `size:<10MB`
  - `type:dir` or `type:file`
  - Combinations: `node_modules size:>500MB`

### 🗄️ 6. SQLite WAL Persistence & Incremental USN Sync
- **Sub-100ms Hydration**: Embedded SQLite engine (WAL mode, memory-mapped I/O) restores trees instantly across sessions.
- **Dual-Tier Change Sync**:
  - *Tier 1 (NTFS USN Journal)*: Reads `FSCTL_READ_USN_JOURNAL` change records in $< 50\text{ ms}$.
  - *Tier 2 (Timestamp Differential Fallback)*: Re-enumerates directory entries for standard user sessions.
  - *In-Place Mutation*: Updates nodes in the arena directly and bubbles size differences to ancestors in $O(\text{depth})$.

---

## How does SpaceOps compare to WinDirStat, TreeSize, and WizTree?

| Feature | SpaceOps | WinDirStat | TreeSize Free | WizTree |
| :--- | :--- | :--- | :--- | :--- |
| **License** | **Open Source (MIT)** | GPL-2.0 (Legacy) | Proprietary / Free | Proprietary / Closed |
| **UI Framework** | **Modern WinUI 3 (Mica)** | Win32 MFC (1990s) | Win32 MFC | Win32 MFC |
| **Rendering Engine** | **Direct2D / Win2D (120 FPS)**| GDI (Slow on 1M+ files) | GDI+ | Direct2D |
| **1M Nodes Memory** | **~55 MB RSS** | ~350 MB | ~200 MB | ~150 MB |
| **Developer Clutter**| **8 Ecosystems (Node, Rust, WSL)**| None | None | None |
| **Incremental Sync** | **USN Journal + Fallback (<50ms)** | Full rescan only | Full rescan only | USN Journal |
| **Safety Protocol** | **Blacklist + Recycle Bin COM** | Direct delete | Direct delete | Direct delete |
| **Network & Privacy**| **100% Local, Zero Telemetry** | Local | Tracking / Ads | Closed source |

---

## Architecture

SpaceOps isolates performance-sensitive filesystem code in native Rust while providing a modern Windows App SDK user experience:

```text
SpaceOps Desktop App (WinUI 3 / C#)
    │
    ├── Win2D CanvasControl (Direct2D Treemap)
    ├── Managed Services (ScanService, IndexService, DevStorageService)
    │
    ▼ P/Invoke (csbindgen C-ABI SafeHandle Bridge)
Native Core (Rust Workspace)
    │
    ├── crates/ffi          (Panic guards, C-ABI export boundary)
    ├── crates/storage-tree (In-memory arena, squarify layout, SQLite WAL, USN sync)
    ├── crates/scanner      (Rayon parallel walk, Win32 NTFS APIs, cluster arithmetic)
    └── crates/common       (Shared domain models, ScanEvent, NodeId)
```

Read the full [Architecture Specification](docs/architecture.md) for data flows and memory layout.

---

## How do I install and run SpaceOps?

### Option 1: Run the Pre-built Binary
When a tagged release is published, the portable archive is attached to [GitHub Releases](https://github.com/ChadaSaiteja/SpaceOps/releases). Extract it and run `StorageIntelligence.exe`.

> **No tagged release is published yet.** Until then, use Option 2 below to build from source.

### Option 2: Build from Source

#### Prerequisites
- Windows 10 (1809+) or Windows 11 (x64 / ARM64)
- [.NET 9 SDK](https://dotnet.microsoft.com/download)
- [Rust Toolchain (1.75+)](https://rustup.rs/)
- Visual Studio 2022 Build Tools (Desktop development with C++)

```powershell
# 1. Clone the repository
git clone https://github.com/ChadaSaiteja/SpaceOps.git
cd SpaceOps

# 2. Build Rust core and WinUI 3 app
.\storage-intelligence\scripts\build.ps1 -Configuration Release

# 3. Launch SpaceOps
& ".\storage-intelligence\app\StorageIntelligence\bin\Release\net9.0-windows10.0.19041.0\win-x64\StorageIntelligence.exe"
```

For complete setup details and debugging instructions, see the [Developer Guide](docs/development.md).

**More documentation:**
- [Developer Storage Reference](docs/developer-storage.md) — every artifact pattern SpaceOps detects
- [Troubleshooting Guide](docs/troubleshooting.md) — fixes for common build and runtime problems
- [Architecture Specification](docs/architecture.md) — data flows and memory layout
- [Safety Model](docs/safety-model.md) — deletion guardrails
- [Changelog](CHANGELOG.md) — release history

---

## How fast is it? Test suites & benchmark results

SpaceOps is verified by automated test suites across both native Rust and .NET managed layers:

```powershell
# Run Rust workspace tests (91 tests)
cd storage-intelligence/core
cargo test --workspace

# Run .NET integration tests (30 tests)
cd ../..
dotnet test storage-intelligence/tests/integration/StorageIntelligence.IntegrationTests.csproj
```

### 1,000,000-Node Synthetic Benchmark Results
```powershell
cargo run --release -p storage-tree --example tree_benchmark
```
- **Build Time (1M nodes)**: `218 ms` (Target: $\le 2.0\text{ s}$) — **$\sim 9\times$ faster**
- **Lookup Latency**: `26.1 ns` (Target: $< 1,000\text{ ns}$) — **$\sim 38\times$ faster**
- **Subtree Top-100 Files**: `45.7 ms` (Target: $< 50\text{ ms}$) — **Met**
- **Memory Footprint**: `55.0 MB RSS` (Target: $< 300\text{ MB}$) — **$5.4\times$ under budget**

---

## What's planned next?

### Completed (v0.1.0)
- [x] High-speed Win32 parallel filesystem scanner with cluster allocation sizing
- [x] Compact in-memory arena storage tree ($O(1)$ lookups, top-N heap queries)
- [x] Hardware-accelerated Win2D squarified treemap with interactive drill-down
- [x] Sub-5ms tokenized search engine (`ext:`, `size:`, `type:`)
- [x] Zero-accident cleanup engine with COM Recycle Bin integration
- [x] Application manager & true storage attribution
- [x] Developer storage intelligence across 8 ecosystems with dormancy analysis
- [x] Embedded SQLite persistence engine and NTFS USN Change Journal incremental sync

### In Progress
- [ ] **Phase 10: Duplicate File Finder & De-duplication**: Multi-tier pipeline (size grouping $\to$ partial head/tail hash $\to$ full cryptographic hashing) with hardlink and junction awareness.

### Planned
- [ ] **Phase 11: Real-Time System Monitoring & Telemetry**: Live disk read/write throughput, drive health metrics, storage growth rate alerts.
- [ ] **Phase 12: Local AI Storage Intelligence**: Structured local metadata explainability answering natural-language storage queries without sending files off-machine.
- [ ] **Distribution**: WinGet package manifest and Microsoft Store packaging.

---

## Frequently Asked Questions

**Is SpaceOps free?**
Yes. SpaceOps is MIT-licensed open-source software. There is no paid tier, no telemetry, and no network access — it runs entirely offline on your machine.

**Will SpaceOps delete my files permanently?**
No. Every deletion routes through the Windows Shell COM API to the Recycle Bin, so you can restore files if you make a mistake. `C:\Windows`, `C:\Program Files`, boot manager paths, and user system profiles are hard-blocked and can never be removed, even in execution mode. A dry-run mode reports reclaimable space without touching the filesystem.

**Is it safe to run while I have projects open?**
Yes. The scanner treats directory junctions, symlinks, and volume mount points as leaves, so it never double-counts or traverses outside your drive. Reparse points are detected and displayed rather than followed.

**How is this different from WinDirStat, TreeSize, or WizTree?**
SpaceOps is free and open source, uses a hardware-accelerated Direct2D treemap instead of slow GDI rendering, holds a 1M-node tree in roughly 55 MB of RAM, detects developer clutter across 8 ecosystems that competitor tools ignore, and supports incremental resync via the NTFS USN Journal instead of a full rescan.

**Does it need internet access?**
No. SpaceOps is 100% local with zero telemetry. Once built, it never makes a network request.

**What are the system requirements?**
Windows 10 version 1809 or later, or Windows 11, on x64 or ARM64. To build from source you also need the .NET 9 SDK, Rust 1.75+, and Visual Studio 2022 Build Tools with the Desktop C++ workload.

**How do I run the tests?**
The Rust workspace suite has 91 tests and the .NET integration suite has 30, for 121 total. See [Test Suites & Benchmarks](#how-fast-is-it-test-suites--benchmark-results) for the exact commands.

---

## Community & Contributing

Contributions are warmly welcomed! Please read our [Contributing Guide](CONTRIBUTING.md) to get started.

- **Report a Bug**: [Open an issue](.github/ISSUE_TEMPLATE/bug_report.md)
- **Suggest a Feature**: [Feature Request template](.github/ISSUE_TEMPLATE/feature_request.md)
- **Report Performance Bottlenecks**: [Performance template](.github/ISSUE_TEMPLATE/performance.md)
- **Improve the Documentation**: [Documentation template](.github/ISSUE_TEMPLATE/documentation.md)
- **Ask Questions**: Open a [support request](SUPPORT.md). *GitHub Discussions is not yet enabled on this repository.*

---

## Is it safe to delete files with SpaceOps?

Because SpaceOps interacts directly with the filesystem, safety is our top priority. Please review our [Safety Model](docs/safety-model.md) to understand our deletion guardrails.

For reporting security vulnerabilities, please refer to our [Security Policy](SECURITY.md). **Do not report security vulnerabilities via public GitHub issues.**

---

## License

SpaceOps is released under the [MIT License](LICENSE).  
Copyright (c) 2026 Saiteja Chada and SpaceOps Contributors.
