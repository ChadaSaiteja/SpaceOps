# ADR-005: Logging Strategy

## Status

Accepted

## Context

Both the Rust core and the C# UI need diagnostic logging for debugging and performance measurement (PRD §17 requires measured, not guessed, performance). Per PRD §18, the application is local-first and privacy-first: no filesystem data leaves the machine by default. Logging must satisfy both needs without adding cross-boundary complexity.

## Problem

How should Rust and C# each log diagnostic information such that logs are useful for debugging a single user session together, capture enough detail for performance diagnosis, and never leave the local machine by default?

## Options Considered

### Option A — `tracing` in Rust, `Microsoft.Extensions.Logging` in C#, both writing to local rolling files in a shared per-user log directory

Advantages:
- Idiomatic per-language tooling; no cross-language logging library coupling required.
- `tracing`'s structured spans support duration/count diagnostics (e.g., scan timing) without manual instrumentation.
- Purely local files by default, satisfying the privacy requirement without extra effort.

Disadvantages:
- Two separate log files rather than one unified stream — acceptable for a desktop app where timestamp correlation is sufficient.

### Option B — Unified pipeline: Rust logs forwarded through FFI into C#'s logger

Advantages:
- Single true unified log stream.

Disadvantages:
- Adds an FFI call per log line, a performance cost in hot paths (e.g., per-file scan logging).
- Added FFI surface complexity for marginal benefit — premature engineering for MVP.

### Option C — No structured logging (Debug output / Console.WriteLine)

Advantages:
- Zero setup.

Disadvantages:
- Unusable for diagnosing real user issues post-release; no levels or persistence; contradicts the "measurable performance" engineering rule.

## Decision

**Option A.** `tracing` in Rust and `Microsoft.Extensions.Logging` in C#, both writing rolling local log files to the same per-user app-data log folder:

```
%LOCALAPPDATA%\StorageIntelligence\logs\
    core.log   (Rust, via tracing)
    app.log    (C#, via Microsoft.Extensions.Logging)
```

Both use timestamp-prefixed lines to allow manual correlation across the two files. Default log level is `info`/`warn` for scan-level events (start, end, item counts, errors). Per-file `debug`/`trace` detail is opt-in only, disabled by default to avoid disk-space and performance overhead during large scans.

## Reason

This satisfies debugging and performance-measurement needs with idiomatic per-language tooling, avoids the FFI performance cost of a unified pipeline, and keeps all diagnostic data local by default per the privacy architecture — without over-engineering a cross-language logging bridge that MVP doesn't need.

## Consequences

### Positive

- Rust and C# logging is independently simple and idiomatic.
- Default log level avoids the performance/disk cost of logging every file during a scan of millions of items.
- Fully local by default, satisfying privacy requirements without extra design.

### Negative

- No single unified log file — debugging a cross-boundary issue requires reading two files and correlating by timestamp.
- If a future need arises for a truly unified stream (e.g., for support-ticket log collection), that will require a follow-up decision.

## Rejected Alternatives

- **Unified FFI-forwarded pipeline (Option B)**: rejected — adds per-log-line FFI overhead and complexity not justified for MVP.
- **No structured logging (Option C)**: rejected — contradicts measurable-performance and diagnosability needs.

## Security Notes

- Log directory is excluded from any future auto-upload/crash-reporting feature unless the user explicitly opts in (PRD §18).
- No credentials, license keys, or auth tokens are ever logged.
- Logs may contain user file paths (inherent to a storage tool) but remain fully local, consistent with the privacy architecture.

## Follow-up

- A support-focused "collect logs for a bug report" feature (zipping both log files with explicit user consent) can be considered later — not required for MVP.
- Revisit if/when a crash-reporting feature is designed (PRD §18 allows optional crash reporting as a server responsibility).

## Addendum (Phase 1 consolidated architecture review)

- The `ffi` crate owns `tracing` subscriber initialization — a single init call made once when the native DLL is loaded, not repeated per exported function. No other crate configures the subscriber.
- Fatal errors and caught panics (ADR-004) are logged through this same `tracing` subscriber before being translated into an FFI error code.
