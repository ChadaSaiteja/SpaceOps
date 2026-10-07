# ADR-003: Rust Core Module Boundaries

## Status

Accepted

## Context

ADR-002 established the initial Rust workspace crates (`ffi`, `scanner`, `common`) but did not define what each crate owns, what it must not do, or how they depend on each other. Without explicit boundaries, FFI/unsafe concerns risk leaking into pure-logic crates, undermining the "independently testable" goal and the ADR-001 isolation of unsafe code.

## Problem

What does each Rust crate own, and how do they depend on one another, such that `scanner` (and future logic crates) remain testable with plain `cargo test` and have zero knowledge of C ABI/FFI concerns, while `ffi` remains the single place unsafe/C-ABI code is allowed to exist?

## Options Considered

### Option A — Strict layered dependency: `common` ← `scanner` ← `ffi`

Advantages:
- Mirrors the FFI isolation goal from ADR-001 exactly: Rust-native logic never touches unsafe/C-ABI concerns.
- `scanner` fully unit-testable in isolation, no FFI mocking needed.
- Enforced by the compiler (Cargo dependency graph), not just convention.

Disadvantages:
- None significant for current scope.

### Option B — Flat, crates depend on each other as needed

Advantages:
- Less upfront discipline required.

Disadvantages:
- Risks circular dependencies as crates grow (e.g., a future `search` crate needing something FFI-shaped).
- No enforced isolation of `unsafe` code, contradicting the "avoid unsafe unless justified" and "clear module boundaries" engineering rules.

## Decision

**Option A.** Strict layering, enforced by Cargo workspace dependency direction:

```
common   (errors, IDs, shared value types — no internal deps)
   ↑
scanner  (traversal, metadata, size calc — depends on common only)
   ↑
ffi      (C ABI, opaque handles, panic boundary, csbindgen — depends on scanner + common)
```

Rules:
- `scanner`'s public API is plain Rust (no raw pointers, no `#[repr(C)]`). All FFI-safety conversion happens only inside `ffi`.
- `common` holds only types genuinely shared by 2+ crates (e.g., error types, `NodeId`). Types used by only one crate stay in that crate — `common` must not become a dumping ground.
- Future logic crates (`storage-tree`, `search`, `cleanup`, added per ADR-002 when their phase begins) sit at the same layer as `scanner`: depend on `common`, depended on by `ffi`. They do not depend on `scanner` directly unless a real data dependency exists at that time.

### Interfaces (illustrative, refined per-phase)

`scanner` exposes plain-Rust functions/types, e.g.:

```rust
fn scan_drive(
    path: &Path,
    cancel: &CancellationToken,
    on_progress: impl FnMut(ScanProgress),
) -> Result<ScanResult, ScanError>;
```

`ffi` wraps these: converts C function-pointer callbacks into the `impl FnMut` progress closure, converts `common::ScanError` into a C error code, and wraps `ScanResult` behind an opaque handle (per ADR-001).

### Initial `common` types (illustrative)

```rust
pub struct NodeId(u64);
pub enum ScanError {
    PermissionDenied(PathBuf),
    NotFound(PathBuf),
    Cancelled,
    Io(std::io::Error),
}
```

Full type design is deferred to the Phase 2 (Filesystem Scanner) and Phase 3 (Storage Tree) component docs — this ADR only fixes crate boundaries, not full schemas.

## Reason

A compiler-enforced layered dependency graph guarantees the ADR-001 isolation goal (unsafe/FFI code confined to one crate) can't silently erode as the codebase grows, without adding process overhead beyond normal Cargo workspace conventions.

## Consequences

### Positive

- `scanner` (and future logic crates) testable in complete isolation from FFI concerns.
- Unsafe code physically confined to `ffi`, satisfying the "avoid unsafe unless necessary and justified" engineering rule.
- New crates have an unambiguous place in the dependency graph when added.

### Negative

- Requires discipline to keep `common` minimal — periodic review needed as it grows.
- Crates that legitimately need to share logic (not just types) across `scanner`-layer crates will need a follow-up decision when that need arises (not anticipated for MVP).

## Rejected Alternatives

- **Flat/ad-hoc dependencies (Option B)**: rejected — risks circular dependencies and unenforced unsafe-code isolation.

## Follow-up

- Full `common` error/type schema to be finalized alongside Phase 2 (Filesystem Scanner) design.
- Placement of future crates (`storage-tree`, `search`, `cleanup`) confirmed at the layering described here when each phase begins.
