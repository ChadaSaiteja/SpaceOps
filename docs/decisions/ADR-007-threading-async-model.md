# ADR-007: Threading & Async Model

## Status

Accepted

## Context

Scans and searches are CPU/IO-heavy and must not block the WinUI 3 UI thread. ADR-001 established the callback/cancellation-token shape across the FFI boundary; ADR-003 established that `scanner` owns traversal logic internally; ADR-004 established that panics must never unwind across FFI. This ADR defines where threads live and how C# async/await coordinates with Rust's internal parallelism.

## Problem

Where does parallelism live (C# or Rust), how does the UI thread stay responsive during long-running scans, and how do progress callbacks safely reach UI-bound state without violating WinUI 3's thread-affinity rules or ADR-004's panic-safety guarantee?

## Options Considered

### Option A — Rust owns all internal parallelism; C# calls a single blocking FFI entry point from a background `Task`

Advantages:
- Rust's internal parallelism (required for PRD's "parallel scanning") stays fully encapsulated; C# never needs to know how many threads Rust uses.
- Simplest possible C ABI shape: one synchronous, blocking call — no async C ABI protocol needed.
- C# gets `async`/`await` ergonomics for free via `Task.Run`, without a native async runtime.

Disadvantages:
- Parks one .NET thread-pool thread for the duration of a scan — acceptable, this is the intended use of `Task.Run` for long-running blocking work.

### Option B — Async C ABI (Rust exposes a polling/callback-based async interface; C# uses a custom awaiter)

Advantages:
- No blocked .NET thread-pool thread.

Disadvantages:
- Requires a reactor/polling protocol across the FFI boundary — significantly more complex.
- No meaningful benefit for a desktop app where one parked thread-pool thread has negligible cost. Over-engineering for MVP.

### Option C — C# manages its own thread pool, calling into Rust per-directory for fine-grained parallelism

Advantages:
- None meaningful.

Disadvantages:
- Reintroduces high-frequency per-call FFI overhead, contradicting ADR-001's low-overhead goal.
- Duplicates parallelism logic across the language boundary and contradicts ADR-003's goal of `scanner` owning traversal logic entirely.

## Decision

**Option A.** Rust (`scanner`) owns all internal parallelism (e.g., via `rayon` or manual threads) behind a single synchronous, blocking C ABI entry point per operation. C# invokes that entry point from a `Task.Run`-wrapped background thread, exposing an idiomatic async API:

```csharp
Task<ScanResult> ScanDriveAsync(string path, IProgress<ScanProgress> progress, CancellationToken ct);
```

Internally this wraps the blocking P/Invoke call in `Task.Run` and bridges the .NET `CancellationToken` to the native cancellation flag established in ADR-001.

Rules:
- Every thread that may invoke the progress callback (not just the initial FFI entry thread) is individually wrapped in the panic guard from ADR-004 — panic-catching happens at each callback call-site inside Rust, not only at the outer entry point.
- Any callback invocation reaching C# is assumed to be on a non-UI thread; C# always re-dispatches UI-bound updates via `DispatcherQueue.TryEnqueue`, never assumes UI-thread affinity.
- Progress callback frequency is throttled internally by `scanner` (time-based, not per-file) to avoid flooding `DispatcherQueue` — the exact interval is a Phase 2 scanner-design detail, not fixed here.

## Reason

This keeps the FFI surface as simple as possible (one blocking call, no async C ABI protocol) while still delivering a responsive, cancellable, non-blocking experience from the C# UI's perspective, and keeps parallelism strategy entirely inside `scanner` where it can evolve without touching the FFI contract.

## Consequences

### Positive

- UI thread never blocks; standard `Task`/`async`-`await` ergonomics on the C# side.
- Rust's internal parallelism strategy can change (thread count, work-stealing, etc.) without any FFI or C# change.
- Panic safety extends correctly to Rust-spawned worker threads, not just the initial entry point.

### Negative

- One .NET thread-pool thread is parked per concurrent long-running operation — acceptable at MVP scale (one scan at a time), would need revisiting if many concurrent operations become common.
- Requires discipline to panic-guard every callback call-site, not just the outer FFI entry — verified via the ADR-004 test extended to worker threads.

## Rejected Alternatives

- **Async C ABI (Option B)**: rejected — added complexity with no meaningful benefit for this workload.
- **C#-managed fine-grained parallelism (Option C)**: rejected — reintroduces FFI call overhead and duplicates traversal-ownership logic against ADR-003.

## Testing Requirements

- Verify UI-thread responsiveness is maintained during an active scan.
- Verify cancellation requested mid-scan halts traversal within a bounded, measured time.
- Extend the ADR-004 panic-boundary test to confirm a panic on a Rust-spawned worker thread (not just the main entry thread) is caught and does not crash the process.

## Performance Requirements

- Cancellation latency and progress-callback throttling intervals are to be measured and set during Phase 2 (Filesystem Scanner) implementation, per the "don't optimize on guesses" engineering rule — no hard numbers fixed in this ADR.

## Follow-up

- Exact progress-callback throttle interval: Phase 2 scanner design.
- Search's threading model (Phase 5): expected to follow this same pattern, confirmed when that phase begins.

## Addendum (Phase 1 consolidated architecture review)

- `ScanDriveAsync`'s native call takes a parameters struct, not just a bare path, so future settings-derived scan options (e.g., exclusion rules, per ADR-006) can be added without changing the FFI entry point's shape — the struct's exact fields are defined when those options are designed (Phase 2 onward), not here.
- This ADR covers one scan (or search) operation at a time; running scan and search concurrently is explicitly out of scope for MVP and not guaranteed to work until designed.
- FFI versioning convention: since the native DLL and C# P/Invoke declarations are built and shipped together as one unit (ADR-001, unpackaged distribution), no semantic-versioning scheme is needed for the C ABI at MVP — a breaking signature change simply requires both sides to be rebuilt and shipped together.
