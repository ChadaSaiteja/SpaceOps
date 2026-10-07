# Application Manager & Storage Attribution — Component Design

Status: **Approved design for Phase 7.**

Governed by: ADR-001 (FFI), ADR-003 (module boundaries), ADR-007 (threading), ADR-009 (StorageTree arena), ADR-013 (Cleanup safety), ADR-014 (Application Manager architecture), PRD §14 (Application Manager).

---

## 1. Problem Statement

Installed software represents a major driver of disk consumption on Windows, but users lack accurate insight into which applications are consuming space. Windows Settings often reports missing or inaccurate sizes, fails to detect secondary data caches in `AppData` or `ProgramData`, and leaves gigabytes of orphaned residue after uninstallers complete.

The **Application Manager & Storage Attribution Engine** solves this by:
1. Providing a unified, fast inventory of both Win32 desktop software and packaged MSIX/Store applications.
2. Cross-referencing application installation paths with the active `StorageTree` arena to compute real cluster-allocated disk usage and file counts.
3. Providing safe, native uninstallation invocation (launching vendor uninstallers or calling package managers).
4. Detecting orphaned leftover directories in user and system cache roots, with safe removal via the Windows Recycle Bin.

---

## 2. Architecture & Pipeline

```text
┌──────────────────────────────────────────────────────────────┐
│                    WinUI 3 Applications View                 │
│      - Size-Sorted App List with Search & Filtering          │
│      - True Disk Size, File Counts, Install Locations        │
│      - Safe Uninstall & Leftover Cleanup Triggers            │
└──────────────┬───────────────────────────────┬───────────────┘
               │                               │
               ▼                               ▼
    ┌──────────────────────┐        ┌──────────────────────┐
    │  C# AppManagerService │        │  Windows.Management  │
    │  (Coordinator & FFI) │        │   .Deployment (WinRT)│
    └──────────┬───────────┘        │   - MSIX Discovery   │
               │                    │   - Package Removal  │
               ▼                    └──────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│                        Rust FFI Layer                        │
│   app_manager_discover_win32()   app_manager_find_leftovers()│
│   app_manager_attribute_size()                               │
└──────────────────────────────┬───────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────┐
│                     storage-tree Crate                       │
│  - Registry Enumeration (HKLM64, HKLM32, HKCU)               │
│  - tree.find_by_path() / Fast On-Disk Traversal              │
│  - Orphaned Cache & Residue Matching                         │
└──────────────────────────────────────────────────────────────┘
```

---

## 3. Data Models

### Application Information (`AppInfo`)
- `id`: Unique stable identifier (e.g., registry subkey name or MSIX package family name).
- `name`: Display name of the application.
- `version`: Version string (e.g. `1.85.1`).
- `publisher`: Publisher or vendor name.
- `install_location`: Absolute root directory of application installation.
- `uninstall_string`: Command line string to execute uninstallation.
- `quiet_uninstall_string`: Silent uninstallation command line (if available).
- `install_date`: Date installed (YYYYMMDD or timestamp).
- `kind`: `Win32` (0) or `Msix` (1).
- `estimated_size`: Estimated size reported in registry (in bytes).
- `actual_size`: Real disk space computed from `StorageTree` or filesystem crawl.
- `file_count`: Real count of files belonging to the installation.
- `is_system_component`: `true` if marked system component or protected OS package.

### Application Leftover (`AppLeftover`)
- `id`: Leftover identifier.
- `app_name`: Associated application name or publisher.
- `path`: Full path to leftover directory (e.g. `%LOCALAPPDATA%\Vendor\AppName`).
- `size`: Reclaimable bytes.
- `file_count`: Total files in leftover directory.
- `location_type`: `LocalAppData`, `AppDataRoaming`, or `ProgramData`.
- `risk_level`: `Low` (standard caches) or `Medium` (user settings).

---

## 4. Safety Model & Invariants

1. **No Pseudo-Uninstalls by Folder Deletion**: The engine will NEVER simply delete an application's install directory to uninstall it. Uninstallation must be mediated by the vendor's registered uninstaller or the Windows Package Manager.
2. **System Component Protection**: System components (`SystemComponent == 1`, Windows Defender, Microsoft Edge runtime, Windows OS updates) cannot have uninstallation triggered.
3. **Leftovers Safe Removal via Recycle Bin**: Orphaned leftover cleanup uses the Windows Recycle Bin COM API (`FOF_ALLOWUNDO`) defined in ADR-013, ensuring zero-risk reversibility.
4. **User Confirmation**: All uninstall actions prompt the user with clear context before executing.

---

## 5. Implementation Phases for Phase 7

- **Step 1 (Rust Core)**: Implement `app_manager` module in `storage-tree` with Win32 registry discovery, path-based tree lookup (`tree.find_by_path`), and leftover discovery.
- **Step 2 (FFI & Bindings)**: Export C-ABI functions in `crates/ffi` with panic guards and auto-generate C# bindings.
- **Step 3 (Managed Services)**: Create `AppManagerService.cs` and ViewModels in `app/StorageIntelligence`.
- **Step 4 (WinUI 3 UI)**: Add Applications management tab / dialog in WinUI app with size sorting, searching, uninstaller launcher, and leftover cleanup.
- **Step 5 (Testing)**: Unit tests in Rust, FFI tests, and .NET integration tests.
