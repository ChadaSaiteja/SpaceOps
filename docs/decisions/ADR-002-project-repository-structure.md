# ADR-002: Project & Repository Structure

## Status

Accepted

## Context

ADR-001 established an in-process Rust `cdylib` + C# P/Invoke bridge for an unpackaged distribution. The repository needs a physical layout that enforces this boundary, keeps Rust modules independently testable (PRD engineering rule), and matches the structure sketched in PRD §27 without over-building for MVP.

Decisions carried in from clarification:

- Root folder name: **`storage-intelligence`** (nests `core/` and `app/`, matching the PRD §27 draft).
- Docs layout: **kept as-is** — `windows-storage-intelligence-agents/` remains the phase/agent prompt library, `docs/decisions/` remains the ADR home. Not merged.

## Problem

Where does Rust core code, C# UI code, FFI glue, tests, and build scripts live, such that: each Rust module is independently buildable/testable, the native DLL output can be copied next to the C# executable at build time (per ADR-001's unpackaged model), and the structure doesn't need reshaping as later phases (search, cleanup, application manager) are added.

## Options Considered

### Option A — Single repo, Rust workspace + C# solution side by side, nested under `storage-intelligence/`

Advantages:
- Matches PRD §27's proposed structure.
- Each Rust crate independently testable (`cargo test -p scanner`).
- New core modules (search, cleanup, etc.) added as new crates later — no restructuring.

Disadvantages:
- Two build systems (`cargo`, `dotnet`) require a small glue script to copy the built DLL into the C# output directory.

### Option B — Single Rust crate (no workspace)

Advantages:
- Less initial ceremony.

Disadvantages:
- Violates "clear module boundaries" / "testable services" engineering rules; scanner/tree/search/cleanup logic would all share one crate, hurting testability by Phase 5–7.

### Option C — Separate repos for Rust core and C# app

Advantages:
- Fully independent versioning/release cadence.

Disadvantages:
- Overkill for MVP; complicates the in-process DLL coupling from ADR-001 with cross-repo version skew risk; no current need for independent release cycles.

## Decision

**Option A.** Repository layout:

```
storage-intelligence/
├── core/                       # Rust workspace
│   ├── Cargo.toml              # workspace manifest
│   ├── crates/
│   │   ├── ffi/                 # cdylib, csbindgen entry point, panic boundary (ADR-001)
│   │   ├── scanner/              # filesystem scanner (Phase 2)
│   │   └── common/               # shared types/errors
│   └── target/
├── app/                        # WinUI 3 / C# solution
│   └── StorageIntelligence.sln
├── tests/
│   └── integration/             # cross-boundary tests
└── scripts/                    # build + copy-native-dll helpers
```

Start minimal: only `ffi`, `scanner`, and `common` crates exist initially. Additional crates (`storage-tree`, `search`, `cleanup`, etc.) are added when their respective phase begins, not scaffolded upfront.

`docs/decisions/` and `windows-storage-intelligence-agents/` remain at the existing repo root, outside `storage-intelligence/`, unchanged.

## Reason

Keeps the Rust/C# boundary from ADR-001 physically enforced, satisfies the "testable services" and "don't implement everything at once" engineering rules by not pre-creating crates for unstarted phases, and avoids the version-skew and process overhead of a multi-repo split with no current justification.

## Consequences

### Positive

- Each Rust module independently buildable and testable from day one.
- Clear, low-ceremony place to add new crates as phases progress.
- No change needed to existing docs folders.

### Negative

- Requires a build/copy script (`scripts/build.ps1` or similar) to keep the native DLL in sync with the C# app's output directory — must be written as part of Phase 1 implementation, not left implicit.
- Two separate build systems in one repo (`cargo`, `dotnet`) means CI must invoke both.

## Rejected Alternatives

- **Single crate (Option B)**: rejected, undermines module boundary and testability rules.
- **Separate repos (Option C)**: rejected, no current need, adds version-skew risk against ADR-001's in-process coupling.

## Follow-up

- Write `scripts/build.ps1` (or equivalent) as part of Phase 1 implementation once all Phase 1 design decisions are approved.
- Revisit crate boundaries when Phase 4 (Search) and Phase 7 (Cleanup) begin — add crates then, not now.
