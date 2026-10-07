# ADR-015: Developer Storage Intelligence & Safe Reclamation Architecture

## Status

Accepted

## Context

Software engineers on Windows are a primary target user demographic for Windows Storage Intelligence (PRD §13, §22). Development environments on Windows are uniquely prone to massive, stealthy disk consumption:
1. **Proliferation of Local Dependencies**: Modern build systems (Node.js, Rust, .NET, Python, Gradle) compile dependencies into localized project directories (`node_modules`, `target`, `bin/obj`, `.venv`). A developer with 20 inactive repositories easily loses 50–150 GB of storage to redundant, duplicate dependencies.
2. **Global Package Caches & Stores**: Tools like NuGet, Cargo, npm, pip, and Gradle download gigabytes of tarballs and prebuilt binaries into user-profile caches that are never automatically pruned.
3. **Massive Virtual Disks & Containers**: Docker Desktop and WSL 2 allocate dynamically expanding `.vhdx` virtual disk images (`ext4.vhdx`) that grow to tens of gigabytes and never automatically shrink when containers or files inside are removed.
4. **Lack of Dormancy Visibility**: Developers do not know which project directories have sat untouched for months while still consuming tens of gigabytes of disk space.

This ADR defines the architectural decisions governing developer storage classification, in-memory arena detection, dormancy analysis, and safe reclamation guardrails.

---

## Decisions

### 1. Three-Tier Developer Artifact Taxonomy

**Decision:** Developer storage artifacts are categorized into three distinct operational tiers:
- **Tier 1: Project Build & Dependency Artifacts (Regenerable)**
  - Folders: `node_modules`, `target/` (Cargo), `bin/` & `obj/` (.NET), `.venv` / `venv` (Python), `build/` & `dist/` (Webpack/Vite), `.gradle/` (Gradle).
  - Risk Level: `Low`.
  - Behavior: Can be safely deleted at any time; automatically recreated on next build or package install (`npm i`, `cargo build`, `dotnet build`).
- **Tier 2: Global Package Caches & Stores**
  - Paths: `%USERPROFILE%\.nuget\packages`, `%USERPROFILE%\.cargo\registry\cache`, `%LOCALAPPDATA%\npm-cache`, `%LOCALAPPDATA%\pip\cache`, `%USERPROFILE%\.gradle\caches`.
  - Risk Level: `Medium`.
  - Behavior: Holds cached packages. Safe to prune; subsequent installations re-download missing assets from network registries.
- **Tier 3: Virtual Machine Disks & Container Engines**
  - Paths: `%LOCALAPPDATA%\Docker\wsl\data\ext4.vhdx`, `%USERPROFILE%\AppData\Local\Packages\*CanonicalGroupLimited*\LocalState\ext4.vhdx`.
  - Risk Level: `High`.
  - Behavior: Stores live container images and Linux root filesystems. Exposes size tracking and compaction guidance; never deletes wholesale without explicit confirmation.

**Reason:**
Provides users with crystal-clear differentiation between what is completely disposable project bloat vs shared developer cache infrastructure.

### 2. High-Speed Detection via In-Memory StorageTree Arena & Global Well-Known Roots

**Decision:** Developer artifact detection operates through a unified dual-source scanner:
1. **Arena Tree Traversal (`tree_detect_dev_artifacts`)**: When a filesystem tree has been scanned into memory, the engine executes a parallel Rayon scan over `nodes` matching known developer artifact directory names (`node_modules`, `target`, `bin`, `obj`, `.venv`, etc.).
   - Uses parent directory names to identify the enclosing project root.
   - Extracts project name and parent metadata in $O(1)$ time.
2. **Global Cache Scanner**: Directly inspects canonical user-profile and local app data cache locations for NuGet, Cargo, npm, pip, Gradle, and Docker.

**Reason:**
Leveraging the in-memory `StorageTree` arena discovers all local project build artifacts in **< 10 milliseconds** without re-crawling the physical filesystem.

### 3. Dormancy Scoring via Filesystem Modification Timestamps

**Decision:** The engine computes a dormancy flag (`is_dormant`) for every detected project artifact:
- Compares the artifact's last modified timestamp against current system time.
- If last modified > 30 days ago, the artifact is flagged as `Dormant` (with 60-day and 90-day severity tiers).
- The UI exposes a "Dormant Projects" quick-filter to highlight abandoned projects holding gigabytes of build bloat.

**Reason:**
Directly answers the core product question: *"What is consuming space that I am no longer actively using?"*

### 4. Zero-Accident Source Guardrail (Never Touch Source Code or .git)

**Decision:** The developer cleanup engine enforces strict, unbypassable safety constraints:
1. **Source Code Immutability**: The engine strictly prohibits targeting project source roots or configuration files (`package.json`, `Cargo.toml`, `*.csproj`, `pyproject.toml`, `*.sln`).
2. **Git Repository Protection**: Directories named `.git` or containing active repository metadata cannot be deleted by developer clean rules.
3. **Windows Recycle Bin Default**: File and directory deletions default to the Windows Shell Recycle Bin (`FOF_ALLOWUNDO`), providing complete reversibility.
4. **Ecosystem Command Guidance**: For global caches, the UI displays the official CLI clean command (e.g. `dotnet nuget locals all --clear`, `cargo cache -a`, `npm cache clean --force`).

**Reason:**
Prevents data loss and ensures developers can freely free up space with absolute confidence.

---

## Consequences

### Positive
- Developers instantly reclaim tens or hundreds of gigabytes of storage from forgotten projects.
- Sub-10ms discovery across scanned trees using the existing `StorageTree` arena.
- Crystal-clear safety boundaries prevent any risk to source code or git history.
- Dormancy tracking helps users systematically prune projects they haven't worked on in months.

### Negative / Trade-offs
- Rebuilding projects after deleting build artifacts requires compilation time on next open. This is clearly communicated in the UI consequence descriptions.
