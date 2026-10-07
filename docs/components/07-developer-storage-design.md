# Developer Storage Intelligence — Component Design

Status: **Approved design for Phase 8.**

Governed by: ADR-001 (FFI), ADR-003 (module boundaries), ADR-007 (threading), ADR-009 (StorageTree arena), ADR-013 (Cleanup safety), ADR-015 (Developer storage architecture), PRD §13 (Developer Storage Analyzer).

---

## 1. Problem Statement

Software developers on Windows accumulate gigabytes to hundreds of gigabytes of duplicate dependencies (`node_modules`), build outputs (`target/`, `bin/obj`), virtual environments (`.venv`), global package caches (NuGet, Cargo, npm, pip), and expanding container images (`Docker ext4.vhdx`).

Because these directories are scattered across various project folders and hidden user-profile locations, developers rarely know:
1. Which projects hold the most build bloat.
2. Which projects haven't been opened or modified in months (dormant).
3. How to safely clean them without risking source code or git history.

The **Developer Storage Intelligence Engine** provides instant visibility, dormancy analysis, and safe 1-click reclamation.

---

## 2. Ecosystem & Target Matrix

| Ecosystem | Target Directories / Files | Category | Reclaimable Behavior | Safety Risk |
| :--- | :--- | :--- | :--- | :--- |
| **Node.js / JS** | `node_modules`, `.next`, `dist`, `build` | Project Artifacts | Recreated via `npm install` / `pnpm install` / `yarn` | 🟢 Low |
| **Node.js Global**| `%LOCALAPPDATA%\npm-cache`, `pnpm-store` | Global Cache | Re-downloaded as needed (`npm cache clean`) | 🟡 Medium |
| **Rust / Cargo** | `target/` | Project Artifacts | Recompiled via `cargo build` | 🟢 Low |
| **Cargo Global** | `%USERPROFILE%\.cargo\registry\cache` | Global Cache | Re-downloaded as needed | 🟡 Medium |
| **.NET / C#** | `bin/`, `obj/` | Project Artifacts | Recompiled via `dotnet build` | 🟢 Low |
| **NuGet Global** | `%USERPROFILE%\.nuget\packages` | Global Cache | Re-downloaded as needed (`dotnet nuget locals all --clear`)| 🟡 Medium |
| **Python** | `.venv`, `venv`, `env`, `__pycache__` | Project Artifacts | Recreated via `python -m venv` / `pip install -r req.txt` | 🟢 Low |
| **Pip Global** | `%LOCALAPPDATA%\pip\cache` | Global Cache | Re-downloaded as needed (`pip cache purge`) | 🟡 Medium |
| **Java / Gradle** | `.gradle/`, `build/`, `.m2/repository` | Project / Global | Rebuilt via `gradle build` / `mvn clean` | 🟡 Medium |
| **Docker / WSL** | `Docker\wsl\data\ext4.vhdx`, distro `.vhdx`| Virtual Disk | Reclaimed via Docker system prune / disk compaction | 🔴 High (Informational) |
| **Git** | `.git\objects\pack` (when bloated) | VCS Repo | Reclaimed via `git gc --prune=now` | 🟡 Medium (Informational) |

---

## 3. Architecture & Query Pipeline

```text
┌──────────────────────────────────────────────────────────────┐
│                    WinUI 3 Dev Storage View                  │
│       - Size-Sorted Developer Artifact Cards                 │
│       - Ecosystem Filters (Node, Rust, .NET, Python, Docker) │
│       - Dormancy Highlighting (> 30 / 60 / 90 days inactive) │
│       - 1-Click Safe Recycle Bin Cleanup                     │
└──────────────┬───────────────────────────────┬───────────────┘
               │                               │
               ▼                               ▼
    ┌──────────────────────┐        ┌──────────────────────┐
    │  C# DevStorageService │        │  Process / Explorer  │
    │  (Coordinator & FFI) │        │  - Open in Explorer  │
    └──────────┬───────────┘        │  - Run Native Tool   │
               │                    └──────────────────────┘
               ▼
┌──────────────────────────────────────────────────────────────┐
│                        Rust FFI Layer                        │
│   dev_catalog_create()            dev_clean_artifact()       │
│   dev_catalog_item() / string()                              │
└──────────────────────────────┬───────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────┐
│                     storage-tree Crate                       │
│  - Arena Tree Scanner (parallel match of target,             │
│    node_modules, bin, obj, .venv, etc.)                      │
│  - Parent project name extraction                            │
│  - Timestamp dormancy evaluation                             │
│  - Global developer cache folder prober                      │
└──────────────────────────────────────────────────────────────┘
```

---

## 4. Safety Guardrails & Invariants

1. **Source Code Immutability**: The engine NEVER deletes source files (`*.rs`, `*.cs`, `*.js`, `*.ts`, `*.py`, `*.cpp`) or project manifests (`Cargo.toml`, `package.json`, `*.csproj`, `pyproject.toml`, `pom.xml`, `build.gradle`).
2. **Git Root Protection**: Folders named `.git` cannot be deleted by automated clean operations.
3. **Reversible Removal (Recycle Bin)**: Manual cleanup uses the Windows Shell Recycle Bin (`FOF_ALLOWUNDO`) by default so developers can undo any deletion.
4. **Dry-Run Simulation**: Supports auditing reclaimable size and file counts before deletion.

---

## 5. Implementation Phases for Phase 8

- **Step 1 (Rust Core)**: Implement `developer.rs` module in `storage-tree` with fast arena tree scan, global prober, and dormancy evaluator.
- **Step 2 (FFI & Bindings)**: Export C-ABI functions in `crates/ffi` with panic guards and auto-generate C# bindings.
- **Step 3 (Managed Services)**: Create `DevStorageService.cs` and `DevArtifactViewModel.cs` in `app/StorageIntelligence`.
- **Step 4 (WinUI 3 UI)**: Add "Dev Storage" button, modal dialog with ecosystem filtering, dormancy badges, and safe cleanup in `MainPage.xaml`.
- **Step 5 (Testing)**: Comprehensive unit tests in Rust, FFI tests, and .NET integration tests.
