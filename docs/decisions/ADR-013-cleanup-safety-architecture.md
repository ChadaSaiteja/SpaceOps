# ADR-013: Cleanup Safety Architecture & Safe Deletion Protocol

## Status

Accepted

## Context

Phase 6 introduces the Cleanup Engine for Windows Storage Intelligence. While storage scanning and visualization are read-only and inherently risk-free, cleanup modifies the filesystem by removing gigabytes of accumulated temporary files, stale caches, and system residue.

A flawed disk cleanup design can cause catastrophic data loss: deleting critical operating system binaries, corrupting user documents, or destroying running application states.

Key requirements:
1. Zero-accident safety model: Protect Windows, Program Files, user libraries, and boot files.
2. Recoverable deletions: Default to Windows Shell Recycle Bin (`FOF_ALLOWUNDO`), never raw unrecoverable `DeleteFileW` without explicit override.
3. Dry-run simulation: Audit candidate size and detect locked files before executing.
4. Total transparency: Users must understand *what* is deleted, *why*, and the *consequences* (PRD §12).

---

## Decisions

### 1. Immutable Protected Path Blacklist

**Decision:** The cleanup engine enforces a hardcoded, unbypassable path blacklist in Rust:
- Root drives (`C:\`, `D:\`)
- `%SystemRoot%` (`C:\Windows`), `System32`, `SysWOW64`, `WinSxS`, `Boot`
- `C:\Program Files`, `C:\Program Files (x86)`
- `%USERPROFILE%`, `Desktop`, `Documents`, `Pictures`, `Music`, `Videos`
- Critical system files (`pagefile.sys`, `swapfile.sys`, `hiberfil.sys`)

Any candidate path that matches, contains, or is an ancestor/descendant of a protected path triggers an immediate `is_protected = true` flag and rejection by the cleanup kernel.

### 2. Windows Shell Recycle Bin Integration

**Decision:** File removal utilizes the Windows Shell COM API (`SHFileOperationW` with `FO_DELETE`, `FOF_ALLOWUNDO`, `FOF_SILENT`, `FOF_NOCONFIRMATION`).
- If an item is accidentally removed, the user can restore it immediately from the Windows Recycle Bin.
- Permanent deletion is only enabled if the user explicitly purges the Recycle Bin rule itself.

### 3. Separation of Detection and Execution

**Decision:** Detection (`cleanup_detect_candidates`) and Execution (`cleanup_execute_rule`) are completely decoupled:
- **Detection**: Queries the in-memory `StorageTree` arena and filesystem targets to compute file counts and byte totals in $< 20\text{ ms}$. No file handles are opened with write/delete access.
- **Execution**: Accepts an explicit `rule_id`, checks safety blacklists, supports `dry_run` flag, handles file-locking gracefully (`ERROR_SHARING_VIOLATION` is recorded as `files_failed` without aborting), and returns a detailed `CleanupReportFfi`.

### 4. Rule-Based Classification with Explanatory Transparency

**Decision:** The engine defines deterministic, rule-based cleanup categories (PRD §12):
- `UserTemp`: User application temporary files older than 24h.
- `SystemTemp`: Windows services temp files.
- `WindowsUpdate`: Staged updates in `SoftwareDistribution\Download`.
- `CrashDumps`: Memory and minidumps from crashes.
- `Thumbcache`: Stale explorer thumbnail cache databases.
- `RecycleBin`: Per-drive recycle bin items.
- `StaleLogs`: Old log files (`.log`, `.old`, `.bak`) in temp directories.

Each category exposes rich explanatory metadata to the UI.

---

## Consequences

### Positive
- Users can confidently reclaim tens of gigabytes of disk space without fear of breaking Windows or losing personal data.
- Accidental deletions are recoverable from the Recycle Bin.
- Locked/in-use files fail gracefully without aborting the batch.
- Complete parity with PRD §12 transparency requirements.

### Negative / Trade-offs
- Sending hundreds of thousands of individual files to the Recycle Bin is slower than raw Win32 `DeleteFileW`. However, safety decisively outweighs microsecond deletion speed.
