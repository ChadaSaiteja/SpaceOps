# Developer Storage Intelligence in SpaceOps

Windows developers face a common problem: SSD storage disappears into hidden package caches, nested dependency trees, container virtual disks, and incremental build artifacts.

Traditional disk cleaners treat these as generic files or ignore them entirely. **SpaceOps** provides dedicated, ecosystem-aware intelligence for developer environments.

---

## 1. Why Developer Storage Disappears on Windows

1. **Nested Package Trees (`node_modules`)**:
   - Modern JavaScript/TypeScript projects frequently install hundreds of MBs in `node_modules`. Abandoned or dormant projects keep gigabytes locked up in deep directory structures.
2. **Container & Linux Virtual Disks (`ext4.vhdx`)**:
   - WSL2 and Docker Desktop allocate dynamically expanding Virtual Hard Disk (`.vhdx`) files. When containers or packages inside WSL2 are deleted, Windows NTFS does *not* automatically shrink the sparse VHDX file on host storage.
3. **Rust Incremental Build Artifacts (`target/`)**:
   - Cargo build artifacts, debug symbols (`.pdb`), intermediate object files, and proc-macro caches routinely grow to 5–20 GB per active workspace.
4. **Python Environments & Bytecode (`venv`, `__pycache__`)**:
   - Virtual environments duplicate Python standard libraries and pip wheel caches across multiple local folders.
5. **.NET & MSBuild Intermediate Output (`bin/`, `obj/`)**:
   - Incremental builds accumulate outdated assembly snapshots and symbols across multiple target frameworks.

---

## 2. Supported Developer Toolchains & Detectors

SpaceOps scans and attributes developer clutter across **8 primary ecosystems**:

| Ecosystem | Detected Targets | Reclamation Impact | Safety Category |
| :--- | :--- | :--- | :--- |
| **Node.js / Web** | `node_modules`, npm cache, pnpm store, yarn cache | High (10–50 GB) | Safe (Re-installable via lockfile) |
| **Rust** | `target/`, cargo registry cache, git checkouts | High (5–30 GB) | Safe (`cargo clean` equivalent) |
| **.NET / C#** | `bin/`, `obj/`, global nuget package cache | Medium (2–15 GB) | Safe (`dotnet clean` equivalent) |
| **Python** | `venv`, `.venv`, `.pytest_cache`, `__pycache__`, pip cache | Medium (2–10 GB) | Safe (Re-installable via requirements.txt) |
| **Java / JVM** | `.gradle/caches`, `.m2/repository` | High (5–20 GB) | Safe (Auto-redownloaded on build) |
| **Containers & WSL** | Docker Desktop VHDX, WSL2 `ext4.vhdx`, buildkit | Critical (20–100+ GB) | Requires Confirmation |
| **Version Control** | Stale `.git/objects/pack` garbage | Low–Medium | Requires Prune Check |
| **IDE & Editors** | VS Code workspace storage, Visual Studio `.vs/` | Medium (1–5 GB) | Safe (Regenerated on project load) |

---

## 3. Dormancy Detection & Smart Recommendations

Rather than recommending that you clean active development projects, SpaceOps analyzes the **timestamp delta**:

- **Active Projects (< 30 days since last modification)**: Highlighted for inspection, but cleanup recommendations default to *unchecked*.
- **Dormant Projects (≥ 30 days since last modification)**: Flagged as prime reclamation candidates.
- **Dormant Projects (> 90 days)**: Prioritized at the top of the reclamation dashboard.

---

## 4. Immutable Safety Guardrails

SpaceOps strictly protects your intellectual property and project source code:

1. **Source Code Immutability**: SpaceOps will *never* delete source code files (`.rs`, `.cs`, `.js`, `.ts`, `.py`, `.cpp`, `.h`, `.go`, `.java`, etc.).
2. **Version Control Protection**: Git metadata files (`.git/config`, `.git/refs`, commit trees) are immutable and strictly excluded from cleanup rules.
3. **Uncommitted Work Safety**: SpaceOps does not touch working directories; cleanup targets only designated artifact folders (`node_modules`, `target`, `bin`, `obj`).
4. **Recycle Bin Routing**: All cleanups are routed to the Windows Recycle Bin via COM, allowing instant restoration if an artifact is ever needed.

---

## 5. Reclaiming WSL2 and Docker VHDX Space

When Docker images or WSL2 files are deleted inside Linux, the physical `ext4.vhdx` file on Windows remains inflated.

SpaceOps identifies the host location of these virtual disks:
- Docker Desktop: `%LOCALAPPDATA%\Docker\wsl\data\ext4.vhdx`
- WSL Distributions: `%LOCALAPPDATA%\Packages\<Distro>\LocalState\ext4.vhdx`

SpaceOps guides users through safe compaction using native Windows disk management tools:

```powershell
# Shutdown WSL instances cleanly
wsl --shutdown

# Optimize virtual disk via Diskpart
# (SpaceOps can assist with safe PowerShell execution)
```
