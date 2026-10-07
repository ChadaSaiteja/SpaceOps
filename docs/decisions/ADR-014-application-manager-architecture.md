# ADR-014: Application Manager & Storage Attribution Architecture

## Status

Accepted

## Context

Phase 7 introduces the Application Management & Storage Attribution Engine for Windows Storage Intelligence (PRD §14). On Windows systems, installed software often represents the single largest category of storage consumption (games, developer environments, design suites, enterprise tools). However, Windows default tools (Windows Settings > Installed Apps) exhibit chronic deficiencies:
1. **Inaccurate / Missing Sizes**: Up to 40% of standard desktop applications report 0 bytes, missing sizes, or outdated `EstimatedSize` registry values that do not reflect true installed disk footprint.
2. **Scattered Footprints**: Modern applications scatter data across multiple locations: installation root (`Program Files`), user caches (`%LOCALAPPDATA%`), shared assets (`%PROGRAMDATA%`), and roaming profiles (`%APPDATA%`).
3. **Uninstall Residue (Leftovers)**: Standard vendor uninstallers frequently leave gigabytes of caches, logs, settings, and temporary files behind in `AppData` and `ProgramData`.
4. **Dual Ecosystem Disconnect**: Traditional Win32 software (Registry-based) and modern packaged apps (MSIX / AppX / Windows Store) require fundamentally different discovery and uninstall mechanisms.

This ADR defines the architectural decisions governing application discovery, storage attribution, safe uninstallation, and leftover detection.

---

## Decisions

### 1. Dual-Ecosystem Hybrid Discovery Model

**Decision:** Application discovery combines native Win32 Registry enumeration (in Rust for ultra-fast, zero-overhead cataloging) with modern WinRT `Windows.Management.Deployment.PackageManager` (in C# for MSIX/Store packages):
- **Win32 Discovery (Rust)**:
  - Scans `HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall` (64-bit native).
  - Scans `HKLM\Software\Wow6432Node\Microsoft\Windows\CurrentVersion\Uninstall` (32-bit compatibility).
  - Scans `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall` (Per-user installations, e.g., VS Code, Chrome, Discord).
  - Filters out system updates (`SystemComponent = 1`, `ParentKeyName` present, Windows Hotfixes).
- **MSIX / AppX Discovery (C# / WinRT)**:
  - Uses `PackageManager.FindPackagesForUser("")` to discover sandbox packages, app family names, and publisher IDs.

**Reason:**
Provides 100% coverage across legacy enterprise software, modern desktop apps, and Windows Store apps without missing any installed software.

### 2. High-Fidelity Storage Attribution via StorageTree Arena

**Decision:** When a drive or directory has been scanned into the in-memory `StorageTree`, the application manager resolves `InstallLocation` directly against the arena tree using hierarchical path resolution (`tree.find_by_path`).
- If an application's install directory exists in the active `StorageTree`, the exact cluster-allocated size, file count, and folder count are resolved in $O(\text{depth})$ time.
- If the directory has not yet been scanned (or is on another drive), a targeted background disk size query calculates the actual allocated size on disk.
- Registry `EstimatedSize` is used only as a last-resort fallback when directories are inaccessible.

**Reason:**
Eliminates misleading 0-byte or inaccurate registry numbers, giving users genuine visibility into which applications are dominating their storage.

### 3. Safe Uninstallation Protocol (Never Raw Folder Deletion)

**Decision:** The application manager strictly rejects arbitrary directory deletion as an "uninstall" mechanism:
- **Win32 Applications**: Execute the vendor's official `UninstallString` or `QuietUninstallString` via Windows Process invocation. If elevated privileges are needed, standard Windows UAC handles the request.
- **MSIX / Store Apps**: Execute `PackageManager.RemovePackageAsync` to cleanly tear down app registration, shortcuts, and sandboxes.

**Reason:**
Deleting application directories manually leaves orphaned COM registrations, registry keys, background services, drivers, and startup tasks that destabilize the operating system.

### 4. Post-Uninstall & Orphaned Leftover Detection

**Decision:** The engine provides a dedicated Leftover Detector that inspects `%LOCALAPPDATA%`, `%APPDATA%`, and `%PROGRAMDATA%` for directories matching known publisher/app names that no longer have an active installed application in the registry or package manager.
- All detected leftovers are marked with safety risk levels (`Low` / `Medium`).
- Removal of leftovers routes exclusively through the Zero-Accident Cleanup Engine (`ADR-013`), using the Windows Recycle Bin (`FOF_ALLOWUNDO`) so users can restore files if needed.

**Reason:**
Reclaims gigabytes of orphaned caches safely while providing a complete undo guarantee.

---

## Consequences

### Positive
- Users gain an accurate, comprehensive, and size-sorted inventory of all software on their system.
- Direct synergy with `StorageTree` turns previously scanned disk data into instant application size breakdowns.
- Zero risk of system corruption from reckless uninstallation.
- Safe, recoverable removal of lingering application leftovers.

### Negative / Trade-offs
- Calling vendor uninstallers opens the vendor's own UI/wizard, which cannot always be automated silently without vendor-specific CLI flags. This is intentional to ensure vendor-sanctioned uninstallation.
