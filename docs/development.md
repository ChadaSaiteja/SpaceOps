# SpaceOps Developer & Setup Guide

This guide explains how to configure your Windows workstation, build the native Rust workspace, compile the WinUI 3 frontend, and run the automated test suites.

---

## 1. Prerequisites

Ensure your system meets the following requirements:

- **Operating System**: Windows 10 (version 1809+ / Build 17763+) or Windows 11.
- **.NET SDK**: .NET 9 SDK (`9.0.100` or newer).
  - Download from: [https://dotnet.microsoft.com/download](https://dotnet.microsoft.com/download)
- **Rust Toolchain**: Stable Rust compiler (1.75+ or newer).
  - Install via `rustup`: `rustup default stable-x86_64-pc-windows-msvc`
- **Visual Studio 2022 Build Tools**:
  - Required workloads:
    - **Desktop development with C++** (MSVC v143, Windows 10/11 SDK).
    - **.NET Desktop Development**.
- **PowerShell**: Windows PowerShell 5.1 or PowerShell 7 Core.

---

## 2. Environment Verification

Run the following in PowerShell to verify toolchains:

```powershell
# Verify .NET 9
dotnet --version

# Verify Rust
rustc --version
cargo --version
```

---

## 3. Quick Build & Run

### Method A: Automated PowerShell Script

```powershell
# Build both Rust Core and WinUI 3 Solution
.\storage-intelligence\scripts\build.ps1 -Configuration Release

# Launch the compiled executable
& ".\storage-intelligence\app\StorageIntelligence\bin\Release\net9.0-windows10.0.19041.0\win-x64\StorageIntelligence.exe"
```

### Method B: Manual CLI Commands

```powershell
# 1. Build Rust FFI dynamic library
cd storage-intelligence/core
cargo build --release -p ffi

# 2. Build and launch the WinUI 3 app
cd ../app
dotnet run --project StorageIntelligence/StorageIntelligence.csproj -c Release
```

---

## 4. Running the Test Suites

SpaceOps maintains complete test suites for both native Rust crates and .NET managed services:

### 1. Rust Workspace Tests (91 Tests)

```powershell
cd storage-intelligence/core
cargo test --workspace
```

### 2. .NET Integration Test Suite (30 Tests)

```powershell
cd storage-intelligence
dotnet test tests/integration/StorageIntelligence.IntegrationTests.csproj
```

### 3. High-Scale 1,000,000-Node Synthetic Benchmark

```powershell
cd storage-intelligence/core
cargo run --release -p storage-tree --example tree_benchmark
```

Expected benchmark results:
- **Build time (1M nodes)**: $\le 250\text{ ms}$
- **Random lookup latency**: $\le 30\text{ ns}$
- **Subtree top-100 files**: $\le 50\text{ ms}$
- **Memory footprint**: $\approx 55\text{ MB}$ RSS delta

---

## 5. Regenerating P/Invoke Bindings (`csbindgen`)

If you add or modify native C-ABI exports in `storage-intelligence/core/crates/ffi/src/lib.rs`:

1. Update `crates/ffi/build.rs` if introducing new types.
2. Compile the `ffi` crate:
   ```powershell
   cd storage-intelligence/core
   cargo build -p ffi
   ```
3. The build script automatically regenerates:
   `storage-intelligence/app/StorageIntelligence/Native/NativeMethods.g.cs`
4. Rebuild the C# project to verify type compatibility:
   ```powershell
   cd ../app
   dotnet build StorageIntelligence.sln
   ```

---

## 6. Debugging Native Code in Visual Studio

To debug both C# WinUI 3 and native Rust code simultaneously:

1. Open `storage-intelligence/app/StorageIntelligence.sln` in **Visual Studio 2022**.
2. Right-click the `StorageIntelligence` project $\to$ **Properties** $\to$ **Debug** $\to$ **General** $\to$ **Open debug launch profiles UI**.
3. Under **Debugger type**, select **Native Only** or **Mixed (Managed and Native)**.
4. Set breakpoints in both C# `.cs` files and Rust `.rs` files.
5. Press **F5** to start debugging.
