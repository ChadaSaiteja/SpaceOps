# ADR-004: Error Handling Strategy

## Status

Accepted

## Context

ADR-001 requires that Rust never unwind/panic across the FFI boundary and deferred the exact panic-boundary convention to a follow-up. ADR-003 placed error types in `common`. The PRD requires that per-file problems (permission denied, locked files, broken junctions/reparse points) are reported as part of scan results rather than aborting the scan, while genuine bugs must never crash the process silently or corrupt state across the FFI boundary.

## Problem

What error model do we use across three tiers — pure Rust logic (`scanner`), the FFI boundary (`ffi`), and the C# UI — so that per-item recoverable issues surface as data, fatal errors surface as structured failures, and panics never unwind into C#?

## Options Considered

### Option A — Two-tier errors: per-item recoverable data + fatal `Result` errors, `thiserror`-based, numeric code + message at FFI boundary

Advantages:
- Matches the PRD requirement that per-file issues (permissions, locked files, junctions) are reported within scan results, not treated as scan failures.
- Keeps the FFI boundary panic-safe per ADR-001.
- Clear separation between "expected, per-item" and "unexpected, scan-level" failures.

Disadvantages:
- Two error paths to keep conceptually distinct — mitigated by keeping per-item issues strictly as result data, never as `Result::Err`.

### Option B — Single flat error enum for everything, including per-file issues

Advantages:
- Simpler type system.

Disadvantages:
- Conflates "show in a details panel" (one locked file) with "scan cannot continue" (bad drive path); forces callers to inspect error content to determine severity; doesn't match how the PRD describes inaccessible files as part of scan results.

### Option C — Result codes only, no structured error type (C-style errno)

Advantages:
- Simplest possible FFI shape.

Disadvantages:
- Discards Rust's strong-typing benefits even in pure-Rust code; scatters magic numbers through internal logic, contradicting the "explicit error handling" and "strong types" engineering rules.

## Decision

**Option A.** Three-tier model:

1. **Per-item recoverable issues** (permission denied, locked file, broken reparse point on one path) are collected into the scan result data (e.g., a list of `InaccessiblePath { path, reason }`). They never abort the scan and never appear as `Result::Err`.
2. **Fatal errors** (invalid drive path, cancellation, out-of-memory) are returned as `Result::Err(ScanError)` from `scanner`, and translated by `ffi` into a numeric error code plus an owned error-message string that C# reads and frees explicitly (same ownership discipline as ADR-001's opaque handles: `ffi` allocates, exposes a `free_error_message` function, C# wraps it in a `SafeHandle`).
3. **Panics** (bugs) are caught at the outermost `ffi` function boundary via `catch_unwind` and converted into a distinct "internal error" code. They never unwind across the FFI boundary. All `ffi`-exported functions go through a single wrapping macro (e.g., `ffi_export! { ... }`) so panic-catching is structural rather than opt-in per function.

Rust-side error enums use `thiserror` for ergonomics in `common`/`scanner`. `ffi` is the only crate performing enum→numeric-code translation, consistent with ADR-003's layering.

### Illustrative types

```rust
// common crate
#[derive(thiserror::Error, Debug)]
pub enum ScanError {
    #[error("invalid path: {0}")]
    InvalidPath(PathBuf),
    #[error("scan cancelled")]
    Cancelled,
    #[error("internal error: {0}")]
    Internal(String), // populated only by the catch_unwind boundary
}

pub struct InaccessiblePath {
    pub path: PathBuf,
    pub reason: InaccessibleReason, // PermissionDenied, Locked, BrokenReparsePoint, etc.
}
```

Full `InaccessibleReason` enum is finalized in the Phase 2 (Filesystem Scanner) design — this ADR fixes the tiering and boundary behavior only.

## Reason

This tiering is the only option that satisfies both the PRD's requirement that scans tolerate per-file problems and ADR-001's requirement that the FFI boundary never lets a panic unwind into C#, while keeping strong typing on the Rust side per the project's engineering rules.

## Consequences

### Positive

- Scans are resilient to permission/locking/junction issues by design, not by convention.
- Panics are structurally caught (via a single macro all exports go through), not dependent on each function remembering to catch them.
- Clear, testable boundary behavior: fatal vs. per-item vs. internal-bug errors are distinguishable by both Rust and C# code.

### Negative

- Requires disciplined use of the `ffi_export!` wrapping macro for every new exported function — a missed wrapping would reintroduce unwind risk. Mitigated by code review and a boundary test (see Testing).
- Error-message string ownership across FFI adds one more `SafeHandle`-style type C# must manage correctly.

## Rejected Alternatives

- **Flat error enum (Option B)**: rejected — conflates per-item and fatal severities.
- **Errno-style codes only (Option C)**: rejected — discards strong typing internally, contradicts engineering rules.

## Testing Requirements

- Unit tests in `scanner` for each `InaccessibleReason` case (permission-denied, broken junction, locked file), fully in pure Rust.
- A dedicated `ffi` test that deliberately panics inside a wrapped export and asserts it surfaces as the internal-error code rather than crashing the test process.

## Security Notes

- Error messages surfaced to C# must not dump raw OS error internals beyond what's actionable to the user; no information beyond the user's own filesystem is exposed.

## Follow-up

- Full `InaccessibleReason` enum finalized alongside Phase 2 (Filesystem Scanner) design.
- Logging strategy (next Phase 1 item) must define how/where these errors are recorded, not just how they're returned.

## Addendum (Phase 1 consolidated architecture review)

- Mid-scan device loss/IO failure (e.g., a removable or network drive disconnecting) is classified under the existing fatal-error tier (`ScanError`, translated to an error code by `ffi`) — no new tier required, just explicit coverage of this real-world Windows case.
- Every fatal error and every panic caught at the `catch_unwind` boundary is logged via `tracing` at `error` level (per ADR-005) before being translated into a C ABI error code, so the log file and the value returned to C# are always consistent.
