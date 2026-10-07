# Contributing to SpaceOps

Thank you for your interest in contributing to **SpaceOps**! We are building an open-source storage intelligence platform for Windows that helps users understand their storage and clean it safely.

---

## Code of Conduct

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md). Please treat all contributors with respect.

---

## Core Principles

When contributing to SpaceOps, keep our core principles in mind:

1. **Safety First**: *"Analyze first. Explain second. Recommend third. Delete last."*
   - Never write code that deletes files silently without user confirmation.
   - Deletions must route through the Windows Recycle Bin via COM (`SHFileOperationW` / `IFileOperation`), never raw unrecoverable deletion unless explicitly configured.
   - Respect the immutable system blacklist (`C:\Windows`, `C:\Program Files`, boot managers, user system roots).
2. **Performance by Default**:
   - Filesystem operations, memory arenas, tree indexing, and treemap layouts belong in the **Rust Core**.
   - UI responsiveness in **WinUI 3** must never block the dispatcher thread.
3. **Zero-Crash FFI Boundary**:
   - Every exported Rust C-ABI function must be wrapped in `std::panic::catch_unwind`. Panics across the FFI boundary cause immediate process aborts.
   - All unmanaged pointers in C# must be managed via `SafeHandle` derivatives (`SafeStorageTreeHandle`, `SafeScanHandle`, `SafeTreemapHandle`).
4. **Local-First & Privacy**:
   - SpaceOps executes 100% locally on the user's machine. Zero telemetry, zero cloud dependencies, zero network queries during scans.

---

## Development Environment Setup

### Prerequisites

- **Operating System**: Windows 10 (version 1809+ / Build 17763+) or Windows 11 (x64 / ARM64).
- **.NET SDK**: .NET 9 SDK (`9.0.100` or newer).
- **Rust Toolchain**: Stable Rust (1.75+ or newer) with `x86_64-pc-windows-msvc` target.
- **Visual Studio 2022 Build Tools**:
  - Desktop development with C++ (MSVC compiler, Windows 10/11 SDK).
- **PowerShell**: PowerShell 5.1 or PowerShell 7+.

### Verifying Toolchains

```powershell
# Check .NET SDK
dotnet --version

# Check Rust compiler and Cargo
rustc --version
cargo --version
```

---

## Building and Running Locally

### 1. Build Everything via Unified Script

SpaceOps provides a convenient PowerShell build script:

```powershell
# Build in Debug configuration
.\storage-intelligence\scripts\build.ps1 -Configuration Debug

# Build in Release configuration
.\storage-intelligence\scripts\build.ps1 -Configuration Release
```

### 2. Manual Step-by-Step Build

```powershell
# 1. Build the Rust native core (compiles crates/common, crates/scanner, crates/storage-tree, crates/ffi)
cd storage-intelligence/core
cargo build --release -p ffi

# 2. Build and run the WinUI 3 Desktop App
cd ../app
dotnet run --project StorageIntelligence/StorageIntelligence.csproj
```

---

## Running the Test Suites

All PRs must pass the test suites before merging:

### 1. Rust Workspace Unit & Integration Tests (91 Tests)

```powershell
cd storage-intelligence/core
cargo test --workspace
```

To run clippy linter:

```powershell
cargo clippy --all-targets -- -D warnings
```

### 2. .NET Integration Tests (30 Tests)

```powershell
cd storage-intelligence
dotnet test tests/integration/StorageIntelligence.IntegrationTests.csproj
```

### 3. Run the 1,000,000-Node Performance Benchmark

```powershell
cd storage-intelligence/core
cargo run --release -p storage-tree --example tree_benchmark
```

Expected baseline: $\le 2.0\text{ s}$ build time, $< 1\text{ µs}$ lookup latency, $< 300\text{ MB}$ RSS delta.

---

## Repository Structure

```text
SpaceOps/
├── .github/                     # GitHub Actions CI/CD workflows and issue templates
├── docs/                        # Architecture specs, ADRs, component designs, and guides
│   ├── components/              # Detailed component technical specifications (01-08)
│   └── decisions/               # Architecture Decision Records (ADR-001 through ADR-016)
├── storage-intelligence/
│   ├── app/                     # WinUI 3 / Windows App SDK desktop application (C#)
│   │   └── StorageIntelligence/
│   │       ├── Controls/        # Hardware-accelerated Win2D CanvasControl treemap
│   │       ├── Native/          # P/Invoke bindings & managed async services
│   │       ├── MainPage.xaml    # Main UI shell (treemap, cleanup, app manager, dev storage)
│   │       └── MainWindow.xaml  # Mica backdrop window shell
│   ├── core/                    # Rust native core workspace
│   │   └── crates/
│   │       ├── common/          # Shared domain models, ScanEvent, NodeId
│   │       ├── scanner/         # Parallel Win32 NTFS directory traversal & MFT reader
│   │       ├── storage-tree/    # In-memory arena, squarify layout, developer & cleanup engines
│   │       └── ffi/             # C-ABI boundary with csbindgen P/Invoke exports
│   ├── scripts/                 # Build and automation scripts
│   └── tests/
│       └── integration/         # .NET xUnit integration test suite
```

---

## Contribution Workflow

1. **Fork the Repository**: Create a personal fork on GitHub.
2. **Create a Feature Branch**:
   ```bash
   git checkout -b feature/my-cool-feature
   # or
   git checkout -b fix/fix-scanner-symlink-handling
   ```
3. **Commit Your Changes**: Follow clear, conventional commit messages:
   - `feat: add Python pip cache detector`
   - `fix: prevent crash when scanning offline network volume`
   - `docs: clarify USN journal synchronization constraints`
   - `perf: optimize squarified treemap aspect ratio calculation`
4. **Run Tests Locally**: Verify that `cargo test --workspace` and `dotnet test` both pass.
5. **Push and Open a Pull Request**: Submit your PR targeting the `main` branch.

---

## Contribution Areas

We actively welcome contributions in these areas:

- **Developer Tool Detectors**: Adding rules for new programming language build artifacts, caches, and package managers (in `crates/storage-tree/src/developer.rs`).
- **Cleanup Safety Rules**: Expanding safe detection rules for stale system artifacts with appropriate risk classifications (in `crates/storage-tree/src/cleanup.rs`).
- **WinUI 3 Enhancements**: UX polishing, keyboard navigation, high-contrast themes, accessibility improvements.
- **Documentation & Guides**: Improving guides, tutorials, and localization.
- **Performance Profiling**: Optimizing parallel traversal or memory layout for disks with 10M+ files.

---

## Getting Help

If you have questions or need guidance before starting a large PR, open a discussion or file a Feature Request issue so we can discuss the architectural design first!
