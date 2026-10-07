# Coding Conventions

**Analysis Date:** 2026-09-22

Two languages, one process: a Rust workspace (`storage-intelligence/core/`) owns scanning and the C ABI, a C# WinUI 3 app (`storage-intelligence/app/`) owns UI and P/Invoke. Conventions below are the patterns already in source, not aspirational style. Cite the nearest ADR when a choice is architectural.

Layering is compiler-enforced (`docs/decisions/ADR-003-rust-module-boundaries.md`):

```
common   (shared types/errors — no workspace deps)
   ↑
scanner  (traversal, metadata, Win32 — depends on common only)
   ↑
ffi      (C ABI, opaque handles, panic boundary, tracing init)
```

C# never talks to `scanner` or `common` directly. It only P/Invokes `ffi.dll` via generated bindings in `storage-intelligence/app/StorageIntelligence/Native/NativeMethods.g.cs` and the wrapper in `storage-intelligence/app/StorageIntelligence/Native/ScanService.cs`.

## Naming Patterns

**Files:**
- Rust source: `snake_case.rs` matching the module (`traversal.rs`, `metadata.rs`, `winfs.rs`). One crate = one `src/lib.rs`; `ffi` also has `build.rs`.
- C# source: `PascalCase.cs` matching the type (`ScanService.cs`, `MainWindow.xaml.cs`). XAML and code-behind share the type name (`MainPage.xaml` + `MainPage.xaml.cs`).
- Generated C#: `*.g.cs` (`NativeMethods.g.cs`). Do not edit; regenerate via `csbindgen` in `storage-intelligence/core/crates/ffi/build.rs`.
- Tests: C# `*Tests.cs` (`ScanServiceTests.cs`). Rust tests live in the same file under `#[cfg(test)]`, not a separate `tests/` crate directory.
- Examples: `examples/benchmark.rs` (performance harness, not a unit test).

**Functions:**
- Rust: `snake_case`. Public API verbs describe the operation (`scan`, `file_metadata`, `cluster_size`, `cancel_token_create`). FFI exports are C ABI names: `scan_drive`, `free_error_message`, `scan_result_destroy`.
- C#: `PascalCase` methods (`ScanDriveAsync`, `ScanDriveBlocking`, `OnProgress`, `Cancel`). Async public APIs end in `Async`. P/Invoke names match the Rust export exactly (`scan_drive`).

**Variables:**
- Rust: `snake_case` locals and fields (`cluster_size`, `inaccessible_count`, `out_error_message`). FFI out-params are prefixed `out_` (`out_result`, `out_error_message`).
- C#: `camelCase` locals (`rawResult`, `errorMessage`, `progressReports`). Private instance fields use `_camelCase` (`_window` in `App.xaml.cs`). Public properties are `PascalCase` (`TotalFiles`, `ErrorCode`).

**Types:**
- Rust structs/enums: `PascalCase` (`ScanEvent`, `ScanError`, `InaccessibleReason`, `CancelHandle`). Newtypes wrap a single field (`NodeId(pub u64)`, `CancellationToken(Arc<AtomicBool>)`). FFI structs are `*Ffi` (`ScanSummaryFfi`, `ScanProgressFfi`) and `#[repr(C)]`. Error-code enums are `#[repr(i32)]` (`ScanErrorCode`).
- C#: `PascalCase` classes (`ScanService`, `ScanException`). DTOs crossing the managed boundary are `readonly record struct` with an `Info` suffix (`ScanSummaryInfo`, `ScanProgressInfo`). Native ownership wrappers are `*SafeHandle` (`CancelTokenSafeHandle`, `ScanResultSafeHandle`). Namespaces: `StorageIntelligence` (app), `StorageIntelligence.Native` (FFI), `StorageIntelligence.IntegrationTests` (tests).

**Constants:**
- Rust: `SCREAMING_SNAKE_CASE` (`FILE_ATTRIBUTE_REPARSE_POINT`, `ERROR_SHARING_VIOLATION`, `PROGRESS_INTERVAL`, `INVALID_FILE_SIZE`).
- C#: none in hand-written code yet; generated P/Invoke uses `__DllName = "ffi"`.

## Code Style

**Formatting:**
- Rust: rustfmt defaults (no `rustfmt.toml` in repo). Edition 2021, workspace `rust-version = "1.75"` in `storage-intelligence/core/Cargo.toml`. 4-space indent, ~100-column wrap, trailing commas on multiline structs/matches.
- C#: SDK-style defaults. No `.editorconfig`, no StyleCop, no `Directory.Build.props`. `ImplicitUsings` enabled, `Nullable` enabled, `AllowUnsafeBlocks` true on the app project. File-scoped namespaces (`namespace StorageIntelligence;`) in every hand-written `.cs` file.
- PowerShell: `$ErrorActionPreference = "Stop"`; `ValidateSet` for configuration (`storage-intelligence/scripts/build.ps1`).

**Linting:**
- No `clippy.toml`, no `#![deny(clippy::...)]`, no C# analyzers package beyond SDK defaults. Treat clippy/rustc warnings as review comments, not CI gates (no CI config in repo).
- Generated C# disables `CS8500` and `CS8981` at file scope in `NativeMethods.g.cs`.
- `unsafe` is allowed in two places only:
  1. `storage-intelligence/core/crates/ffi/src/lib.rs` — Rust↔C# C ABI.
  2. `storage-intelligence/core/crates/scanner/src/winfs.rs` — Win32 calls with no `std` equivalent (`GetCompressedFileSizeW`, `GetDiskFreeSpaceW`).
- C# `unsafe` is confined to `StorageIntelligence.Native` (`ScanService.cs` and generated `NativeMethods.g.cs`). UI code (`App`, `MainWindow`, `MainPage`) stays safe.

**Visibility:**
- `scanner` public surface is re-exported from `lib.rs` (`pub use traversal::{scan, ScanEvent, ...}`). `winfs` is a private `mod`.
- C# P/Invoke and `SafeHandle` types are `internal`. `ScanService`, `ScanException`, and the `*Info` records are `public` — that is the only C# API tests and UI should call.

## Import Organization

**Order (Rust, observed in `traversal.rs` / `ffi/src/lib.rs`):**
1. `crate::...` (same-crate modules)
2. Workspace crates (`common`, `scanner`)
3. External crates (`rayon`, `windows`, `tracing`)
4. `std::...` (grouped by module, one `use` per module)

Do not glob-import except `rayon::prelude::*`. Prefer explicit paths (`common::ScanError`, `scanner::scan`).

**Order (C#, observed in `App.xaml.cs` / `ScanService.cs`):**
1. `System.*` (when not covered by implicit usings)
2. `Windows.*` / `Microsoft.UI.Xaml.*` (WinUI)
3. Project namespaces (`StorageIntelligence.Native`)

Hand-written files use file-scoped namespaces. Generated `NativeMethods.g.cs` uses a block namespace — leave it.

**Path Aliases:**
- None. No `extern crate`, no C# `global using` beyond the test project's `<Using Include="Xunit" />` and SDK implicit usings.

## Error Handling

Three tiers (`docs/decisions/ADR-004-error-handling-strategy.md`). Do not collapse them.

**Tier 1 — per-item, never `Err`:** permission denied, locked file, broken reparse. Recorded as `ScanEvent::Inaccessible` / `InaccessiblePath`. Scan continues.

```rust
// storage-intelligence/core/crates/scanner/src/traversal.rs
Err(err) => {
    ctx.events.push(ScanEvent::Inaccessible {
        parent_id: dir_id,
        path: path.to_path_buf(),
        reason: classify_io_error(&err),
    });
    ctx.inaccessible_count.fetch_add(1, Ordering::Relaxed);
    return (0, 0, 0);
}
```

**Tier 2 — fatal `Result::Err(ScanError)`:** invalid root, cancellation, device lost. Defined with `thiserror` in `storage-intelligence/core/crates/common/src/lib.rs`:

```rust
#[derive(thiserror::Error, Debug)]
pub enum ScanError {
    #[error("invalid path: {0}")]
    InvalidPath(PathBuf),
    #[error("scan cancelled")]
    Cancelled,
    #[error("device lost during scan: {0}")]
    DeviceLost(PathBuf),
    #[error("internal error: {0}")]
    Internal(String),
}
```

`scanner` returns `Result<(Vec<ScanEvent>, ScanSummary), ScanError>`. `ffi` is the only crate that translates the enum into a numeric `ScanErrorCode` (`Ok=0`, `InvalidPath=1`, `Cancelled=2`, `DeviceLost=3`, `Internal=4`) plus an owned UTF-16 message C# must free via `free_error_message`.

**Tier 3 — panics:** bugs. Caught at every `#[no_mangle]` export with `catch_unwind` / `catch_ffi_panic`. Never unwind into C#. Convert to `ScanErrorCode::Internal` and log at `error`.

```rust
// storage-intelligence/core/crates/ffi/src/lib.rs
fn catch_ffi_panic<R>(default: R, f: impl FnOnce() -> R + std::panic::UnwindSafe) -> R {
    match std::panic::catch_unwind(f) {
        Ok(v) => v,
        Err(_) => {
            init_logging_impl_once();
            tracing::error!("panic caught at FFI boundary; converting to internal error code");
            default
        }
    }
}
```

**C#:** non-zero native codes become `ScanException` (sealed, carries `ErrorCode` + message). Do not throw `TaskCanceledException` for native cancel — `ScanDriveAsync` deliberately does **not** pass the token to `Task.Run`, so a cancelled scan always surfaces as `ScanException` with code `2`.

```csharp
// storage-intelligence/app/StorageIntelligence/Native/ScanService.cs
if (code != 0)
{
    string message = errorMessage != null ? new string((char*)errorMessage) : "unknown scan error";
    if (errorMessage != null)
    {
        NativeMethods.free_error_message(errorMessage);
    }
    throw new ScanException(code, message);
}
```

**IO classification:** map `ErrorKind::PermissionDenied` → `InaccessibleReason::PermissionDenied`, Win32 `32` (`ERROR_SHARING_VIOLATION`) → `Locked`, everything else → `Other(String)`. Do not invent new reasons without updating both `common` and tests.

**Do not:** `unwrap()` / `expect()` on fallible IO in production paths (tests may unwrap). `lock().unwrap()` on internal mutexes is accepted — a poisoned mutex is a bug, which the FFI panic boundary will catch.

## Logging

**Framework:**
- Rust: `tracing` 0.1 + `tracing-subscriber` 0.3 (`env-filter`) + `tracing-appender` 0.2. Subscriber is initialized **once** in `ffi` (`LOGGING_INIT: Once`), never in `scanner` or `common` (`docs/decisions/ADR-005-logging-strategy.md`).
- C#: ADR-005 specifies `Microsoft.Extensions.Logging` writing `app.log`. **Not implemented yet** — do not `Console.WriteLine` as a substitute when adding it. Target path: `%LOCALAPPDATA%\StorageIntelligence\logs\app.log`.

**Patterns:**
- Default level is scan-level `info`/`error`. Per-file `debug`/`trace` is opt-in only (hot path).
- Log fatal errors **before** translating to an FFI code:

```rust
Err(err) => {
    tracing::error!(error = %err, "scan failed");
    set_error_message(out_error_message, &err.to_string());
    match err { /* ScanErrorCode */ }
}
```

- Caught panics: `tracing::error!("panic caught at FFI boundary; converting to internal error code")`.
- Rolling file: `tracing_appender::rolling::daily(log_dir, "core.log")`, non-blocking writer, ANSI off. Guard is `Box::leak`'d because the cdylib lives for the process lifetime.
- Never log credentials, tokens, or anything that would leave the machine. Paths in logs are expected (this is a storage tool) and stay local.

## Comments

**When to Comment:**
- Module-level `//!` docs on every Rust file, citing the governing ADR and the crate's job.
- `///` on every `unsafe` FFI export, including a `# Safety` section that states pointer ownership and double-free rules.
- Non-obvious constraints: why `Task.Run` does not take the cancellation token; why `SendPtr` uses a getter instead of field access (RFC 2229 capture); why reparse points are leaves.
- XML `/// <summary>` on public C# types and methods. Keep them one or two sentences; reference the ADR number.

**When not to:**
- Do not narrate what the next line does (`// increment counter`).
- Do not edit comments inside `NativeMethods.g.cs`.
- Template WinUI comments in `MainPage.xaml.cs` / `MainWindow.xaml.cs` are leftover scaffolding — replace them when that file gains real logic, do not copy the "To learn more about WinUI" boilerplate into new files.

## Function Design

**Size:** Keep functions to one responsibility. `scan` (~55 lines) sets up context and the root event; `walk_dir` enumerates one directory and fans out; `process_entry` classifies one path. If a signature grows past ~5 parameters, bundle shared state (`ScanContext` in `traversal.rs`) rather than adding more args.

**Parameters:**
- Rust public APIs take borrowed inputs (`&Path`, `&CancellationToken`, `&(dyn Fn(ScanProgress) + Send + Sync)`). Progress interval is a `Duration`; production FFI uses `PROGRESS_INTERVAL = 100ms`, tests pass `Duration::ZERO`.
- C# public APIs take `string path`, optional `CancellationToken`, optional `IProgress<ScanProgressInfo>?`. Never block the UI thread — `ScanDriveAsync` always `Task.Run`s the blocking P/Invoke (`docs/decisions/ADR-007-threading-async-model.md`).
- Callbacks arriving from Rust are on a background thread. UI updates must go through `DispatcherQueue.TryEnqueue`. Do not touch XAML from `OnProgress`.

**Return Values:**
- Rust: `Result<T, ScanError>` for operations that can fatally fail; plain structs/tuples otherwise. FFI exports return `i32` error codes (`0` = success) and write owned handles/messages through out-pointers.
- C#: `Task<ScanSummaryInfo>` for the public scan API; `void` for native callbacks. Prefer `readonly record struct` over mutable DTOs for values that cross the native boundary.

**Ownership across FFI (ADR-001):**
- Every `*_create` has exactly one `*_destroy`. Wrap the pointer in a `SafeHandle` subclass (`ownsHandle: true`) so Dispose/finalizer releases it.
- Error messages: Rust `Box<[u16]>` → C# `new string((char*)ptr)` → `free_error_message`. Null message is a no-op on free.
- Do not serialize the full tree across FFI. Scanner streams events; Phase 3 (Storage Tree) will query by node. Today only `ScanSummaryFfi` is exposed.

## Module Design

**Exports:**
- `common`: only types shared by 2+ crates (`NodeId`, `CancellationToken`, `ScanError`, `InaccessiblePath`, `InaccessibleReason`). Do not dump scanner-only types here (`docs/decisions/ADR-003-rust-module-boundaries.md`).
- `scanner`: plain Rust. No raw pointers, no `#[repr(C)]`, no `tracing` subscriber init. Public: `scan`, `ScanEvent`, `ScanProgress`, `ScanSummary`, metadata structs.
- `ffi`: `crate-type = ["cdylib"]`. All `#[no_mangle] pub extern "C"` functions. `csbindgen` generates C# into `app/StorageIntelligence/Native/NativeMethods.g.cs`. If csbindgen cannot express a signature, hand-write that one binding (ADR-001 fallback) — do not fight the generator.
- C#: `ScanService` is a `public static` façade. UI pages are `sealed partial` (`MainPage`, `MainWindow`). App is `partial class App : Application`.

**Barrel Files:**
- Rust `lib.rs` files are thin: module declarations + `pub use` of the public API (`scanner/src/lib.rs`). Do not put traversal logic in `lib.rs`.
- No C# barrel/`Usings.cs`. Each file owns its namespace.

**XAML:**
- Window hosts a `Frame` (`RootFrame`) and navigates to `MainPage` on startup. Put page UI in the page, not the window (`MainWindow.xaml.cs`).
- Unpackaged WinUI 3 (`WindowsPackageType=None`). Copy `ffi.dll` next to the exe via the `CopyNativeFfiDll` MSBuild target after Build — duplicate that target in any new test/app project that P/Invokes native code.

**Stateless Rust (ADR-006):** `scanner` does not read config files. Any future setting that affects a scan is passed as a function argument. C# will own `%LOCALAPPDATA%\StorageIntelligence\settings.json` when settings land.

---

*Convention analysis: 2026-09-22*
