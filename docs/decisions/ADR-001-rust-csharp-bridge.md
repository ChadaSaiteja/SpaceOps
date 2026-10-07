# ADR-001: Rust ↔ C# Communication Bridge

## Status

Accepted

## Context

The application splits work between a WinUI 3 / C# UI layer and a Rust native core (per PRD §4). Every subsequent Phase 1 decision (project structure, threading model, error handling, async model) depends on how these two layers communicate. The PRD (§24) explicitly requires this decision to be discussed — not assumed — covering performance, maintainability, debugging, packaging, and security.

Constraints established for this decision:

- Distribution model: **unpackaged** (plain exe + DLLs), not MSIX. This favors simplicity and unrestricted filesystem access over store-managed deployment.
- Elevation model: the app **never requires elevation for scanning/search**. Elevation is **optional and user-initiated** (restart-as-admin) to reveal protected system paths. Standard-user scans report inaccessible/protected paths as such rather than failing.
- Cleanup/uninstall (Phase 7/8) may need a different privilege boundary later — explicitly deferred, not solved here.

## Problem

WinUI 3 needs to trigger long-running, CPU/IO-heavy Rust operations (drive scans, size aggregation, search) and receive back progress events, cancellation acknowledgement, one-shot results, and large hierarchical tree data — without blocking the UI thread or corrupting state across the language boundary.

## Options Considered

### Option A — In-process C ABI (Rust `cdylib` + C# P/Invoke)

Advantages:
- Highest performance — no IPC/serialization overhead for the call path itself.
- Single deployable unit (one native DLL alongside the C# app); matches the unpackaged distribution decision cleanly.
- Simplest process/lifecycle model — no second process to launch, monitor, or restart.

Disadvantages:
- FFI memory-ownership and thread-marshaling discipline required (mitigated below).
- No built-in process/privilege isolation (acceptable since elevation is deferred/out of scope for this ADR).

### Option B — Separate process + IPC (named pipes / local gRPC)

Advantages:
- Enables privilege separation (useful for a future elevated helper for delete/uninstall).
- Independent debugging/crash isolation per process.

Disadvantages:
- Serialization + IPC overhead on every call — hurts high-frequency interactive operations (hover, drill-down, search-as-you-type).
- Significant added complexity: wire protocol, versioning, process lifecycle, crash/restart handling.
- More installation/antivirus/firewall friction, without a clear MVP need.

### Option C — WinRT component authored in Rust (via `windows-rs`)

Advantages:
- Looks native to C#; integrates with WinRT async patterns (e.g., `IAsyncActionWithProgress`).

Disadvantages:
- WinRT metadata generation from Rust is immature and fragile today.
- High tooling risk for no clear MVP benefit over Option A.

### Rejected without deep comparison

- **Tauri-style bridge**: designed for Rust↔WebView(JS), not Rust↔WinUI3/XAML — wrong shape for this stack.
- **Embedding the CLR inside Rust**: inverts natural ownership (UI should own the process), no benefit here.

## Decision

**Option A: in-process Rust `cdylib` exposed via a stable C ABI, called from C# via P/Invoke.**

Binding generation: use **csbindgen** to generate P/Invoke signatures from the Rust `cdylib` source, reducing hand-written `unsafe` glue and keeping the C# and Rust sides in sync at build time. Hand-written C ABI + manual `cbindgen` header remains the fallback if csbindgen proves insufficient for a specific signature shape.

Concrete design requirements adopted as part of this decision:

1. **Opaque handles** for long-lived native objects (e.g., `ScanHandle`, `TreeHandle`). C# never touches Rust memory layout directly. Every `*_create` has exactly one corresponding `*_destroy`, wrapped in a C# `SafeHandle` subclass so handle lifetime is tied to .NET's finalization/dispose guarantees rather than manual discipline alone.
2. **Batched/on-demand tree queries**, not bulk serialization. The full storage tree stays resident in Rust memory; C# queries it via functions like "children of node X" or "top N by size," paginated as needed. No per-node marshaling and no whole-tree snapshot copy.
3. **Explicit thread-marshaling rule**: Rust invokes progress/cancellation-acknowledgement callbacks from a background thread. C# is responsible for dispatching any UI updates from that callback onto the UI thread via `DispatcherQueue`. Rust must never assume it is safe to touch UI state directly.
4. **Cancellation** via an atomic flag/token passed into Rust at scan start, polled periodically during traversal.

## Reason

This is the simplest option that meets MVP performance and interactivity needs (fast hover/drill-down/search require in-process calls, not IPC round-trips), matches the unpackaged distribution decision, and doesn't introduce infrastructure (a second process, a wire protocol) that the deferred elevation use case doesn't yet justify. The specific risks of in-process FFI (memory ownership, thread affinity) are addressed with concrete, enforceable rules rather than "be careful" conventions.

## Consequences

### Positive

- Fast, low-latency calls suitable for interactive UI (search-as-you-type, treemap hover/drill-down).
- Single native DLL to build, ship, and version alongside the C# app.
- Handle/ownership rules are enforceable in code review (SafeHandle pattern) rather than relying on discipline alone.

### Negative

- No process-level fault isolation: a Rust panic/crash in the core takes down the whole app (must be mitigated with panic-boundary handling at the FFI layer — every exported function catches panics and converts them to error codes, never unwinds across the FFI boundary).
- No privilege separation available today; if Phase 7/8 (cleanup/uninstall) needs elevated operations, that will require a follow-up ADR to introduce a second, separately-elevated process for that narrow purpose.
- csbindgen adds a build-time code-generation dependency; if it becomes a blocker for a specific API shape, falls back to hand-written bindings for that case.

## Rejected Alternatives

- **IPC/separate process (Option B)**: deferred, not rejected outright — revisit specifically for a future elevated cleanup/uninstall helper, not as the primary scan/search path.
- **WinRT-authored-in-Rust (Option C)**: rejected due to tooling immaturity.
- **Tauri-style / CLR-in-Rust**: rejected, wrong shape for this stack.

## Follow-up

- Phase 7/8 (Cleanup Engine, Application Manager): revisit whether an elevated helper process is needed for protected-path deletion or uninstall operations, and if so, produce ADR-00X for that specific IPC boundary.
- Define the exact panic-boundary / error-code convention for FFI exports as part of the Phase 1 error-handling design (separate design doc, not this ADR).
- Define the exact batched query API shape (function signatures) as part of Phase 3 (Storage Tree) design.
