# ADR-006: Configuration Strategy

## Status

Accepted

## Context

The application needs persisted user settings (PRD §21 MVP item #18, "Basic settings") — e.g., last-selected drive, UI preferences. ADR-002 fixed distribution as unpackaged, which rules out `ApplicationData.LocalSettings`. Per the module-boundary philosophy in ADR-003, Rust crates should remain stateless between calls; configuration is a UI-layer concern.

## Problem

Where and how should user settings be persisted locally, such that C# owns the settings surface, Rust remains stateless, and the mechanism survives app restarts without requiring the app to be packaged?

## Options Considered

### Option A — JSON settings file in `%LOCALAPPDATA%\StorageIntelligence\settings.json`, owned entirely by C#

Advantages:
- Simple, human-inspectable, no dependency beyond `System.Text.Json`.
- Matches the unpackaged distribution decision (ADR-002) — no `ApplicationData` API needed.
- Rust never reads/writes config; any scan-affecting setting is passed as an explicit call parameter, keeping `scanner` stateless and testable per ADR-003.

Disadvantages:
- Manual schema versioning needed if the settings shape changes — mitigated with a `version` field and a migration function added only when a breaking change actually occurs.

### Option B — SQLite-backed settings, reusing the scan-data database

Advantages:
- Single storage mechanism for everything.

Disadvantages:
- Overkill for a handful of key-value preferences.
- Unnecessarily couples app-settings lifecycle to the scan database's lifecycle; PRD §10's schema/indexing concerns are about scan data, not preferences.

### Option C — Windows Registry

Advantages:
- Traditional Windows mechanism.

Disadvantages:
- Harder to inspect/debug/back up than a plain file.
- No benefit over a JSON file for an unpackaged app; the PRD only calls for registry access when reading *other* applications' uninstall entries (Phase 8), not for this app's own settings.

## Decision

**Option A.** A JSON settings file at `%LOCALAPPDATA%\StorageIntelligence\settings.json`, owned entirely by the C# layer, using `System.Text.Json`, written with an atomic write-to-temp-then-rename pattern. Rust never persists configuration; any configuration relevant to a scan (e.g., future exclusion rules) is passed explicitly as parameters into the relevant Rust function call.

### Illustrative shape (MVP-minimal)

```json
{
  "version": 1,
  "lastSelectedDrive": "C:\\",
  "theme": "system",
  "treemap": { "colorScheme": "byExtension" }
}
```

## Reason

A plain JSON file is the simplest mechanism that satisfies persistence, local-first/privacy requirements, and the unpackaged distribution constraint, without introducing a database dependency for what is a small set of key-value preferences, and without violating the stateless-Rust-core boundary established in ADR-003.

## Consequences

### Positive

- No added dependency beyond the .NET standard library's JSON support.
- Human-inspectable and easy to debug or hand-edit during development.
- Keeps Rust crates stateless and independently testable.

### Negative

- Requires a manual atomic-write pattern to avoid corruption on crash mid-write.
- Requires a version field and migration path once the schema needs a breaking change (not built upfront, added when first needed).

## Rejected Alternatives

- **SQLite-backed settings (Option B)**: rejected — unnecessary coupling to the scan-data database lifecycle.
- **Windows Registry (Option C)**: rejected — no benefit over a JSON file for this use case.

## Follow-up

- Cleanup rule preferences and developer-storage exclusion rules will extend this settings file when Phase 6/7 design happens — not designed now.
- Settings UI (the screen itself) is out of scope for this ADR; only the persistence mechanism is decided here.
