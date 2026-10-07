# Troubleshooting SpaceOps

This document provides solutions to common questions, platform behaviors, and error messages in SpaceOps.

---

## 1. Scanner & Permission Questions

### Why does SpaceOps request or recommend Administrator privileges?
- **Standard User Mode**: SpaceOps functions fully as a standard, non-elevated user. It uses Win32 kernel directory traversal (`FindFirstFileExW`) to scan files accessible to your user account.
- **Administrator Elevation (Tier 1)**: When launched as Administrator, SpaceOps can access the raw NTFS Master File Table (MFT) and NTFS USN Change Journal (`FSCTL_READ_USN_JOURNAL`). This enables faster scanning of whole drives and sub-50ms incremental change synchronization.
- **Inaccessible Folders**: If running without elevation, system-protected folders (such as `System Volume Information`) will be reported with an `Inaccessible` status and skipped safely without crashing.

### How does SpaceOps handle paths longer than 260 characters (`MAX_PATH`)?
SpaceOps utilizes native Win32 extended-length path formatting (`\\?\` prefix) throughout both the Rust scanner and C# managed layer. It scans deeply nested paths (such as deeply nested `node_modules` or Java package hierarchies) exceeding 260 characters without `PathTooLongException` errors.

### Why are some files skipped with a "Locked" or "Sharing Violation" status?
Files that are opened exclusively by another active process (such as SQL Server database files or active virtual machine disks) return Win32 `ERROR_SHARING_VIOLATION`. SpaceOps captures the file size from directory metadata and continues scanning without blocking worker threads.

---

## 2. Build & Runtime Errors

### Error: `Unable to load DLL 'ffi.dll' or one of its dependencies`
- **Cause**: The unmanaged Rust core (`ffi.dll`) has not been compiled or is not present in the application's output directory.
- **Solution**:
  ```powershell
  cd storage-intelligence/core
  cargo build --release -p ffi
  ```
  The MSBuild target in `StorageIntelligence.csproj` automatically copies `ffi.dll` to the bin folder on build.

### Error: `A compatible .NET SDK was not found`
- **Cause**: SpaceOps targets .NET 9 (`net9.0-windows10.0.19041.0`). If multiple .NET versions are installed or `global.json` points to an older SDK, the build may fail.
- **Solution**:
  Verify your installed SDKs:
  ```powershell
  dotnet --list-sdks
  ```
  Ensure .NET 9 SDK (e.g. `9.0.100` or newer) is present on your system path.

---

## 3. Filesystem Specifics

### Does SpaceOps support ReFS, FAT32, exFAT, and Network Shares?
- **NTFS**: Full support (Parallel traversal, cluster allocation, MFT reader, USN Journal sync).
- **ReFS / exFAT / FAT32**: Full support via Win32 parallel traversal and timestamp differential sync. (Direct MFT and USN Journal features are NTFS-specific and cleanly bypassed).
- **Network Drives (SMB/UNC)**: Supported via Win32 traversal. Progress may depend on network latency.

### How do I report a new issue?
If your issue is not listed here, please file an issue using our [Bug Report Template](https://github.com/ChadaSaiteja/SpaceOps/issues/new?template=bug_report.md).
