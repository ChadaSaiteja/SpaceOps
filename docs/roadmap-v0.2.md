# v0.2 Roadmap: Fluent Sidebar UI, Safe Releases, Discoverability

Working order. Each task ends with a verification gate; do not start the next task until the gate passes.

## Baseline (measured before any changes)
- Rust workspace: 91 tests pass (ffi 18, scanner 14, storage-tree 42, common/other 17); `cargo fmt --check` clean.
- C# build: `dotnet build -c Release` succeeds, 0 warnings, 0 errors (SDK 9.0.318, installed per-user at `%USERPROFILE%\.dotnet`; add it to PATH and set `DOTNET_ROOT`).
- C# integration tests: 30 pass locally (`tests/integration`). Total 91 + 30 = 121, matching the README badge. They were removed from CI; Task 0 restores them.
- Landing page: redesigned and merged (PR #5).

## Task 0: Safety net (do first)
- [x] .NET SDK 9.0.318 available locally; app builds and 30 integration tests pass.
- [x] Re-enable C# integration tests in CI (or a separate non-blocking job) so FFI regressions are caught.
- [x] Add a `smoke` check: launch app, scan a temp folder, assert tree is non-empty. (`tests/integration/SmokeTests.cs` runs scan -> tree -> treemap -> search -> dev storage -> dry-run-only cleanup on a temp folder; CI adds a non-blocking 8 s launch check that reports liveness and `crash.log`.)
- Gate: `cargo test --workspace` green AND `dotnet build -c Release` green AND integration tests green.

## Task 1: Theme foundation
- [x] `Styles/Theme.xaml` resource dictionary: accent `#00F5D4`, blue `#3A86FF`, purple `#9D4EDD`, pink, amber; dark surfaces.
- [x] Single source of truth for category colors (today duplicated in `TreemapService.cs` and the XAML legend).
- [x] Replace inline hex values (`#FF9800`, `#10B981`, `#3B82F6`).
- Gate: app builds; visual diff of main screen.

## Task 2: Fluent Sidebar shell
- [x] `NavigationView` (left, expanded) with: Overview, Treemap, Cleanup, Developer, Applications.
- [x] Slim header: target path, Scan, Sync index. Bottom status bar kept.
- [x] Convert the three modal overlays into pages (they currently hide the treemap).
- [x] Drive-usage card at the bottom of the sidebar.
- Gate: every existing feature still reachable; no code path removed.

## Task 3: Overview page
- [x] Usage donut by category, top 5 consumers, "Reclaimable now" total, quick actions.
- Gate: numbers match the scanned tree.

## Task 4: Treemap polish
- [x] Rounded tiles, subtle gradient, folder vs file distinction, hover glow, details panel matching the mockup.
- Gate: layout hit-testing unchanged (parity tests); 60 FPS on a 1M-node tree.

## Task 5: Cleanup unification
- [x] One candidate list across system junk, dev artifacts, and app leftovers; one reclaimable total; dry-run and Recycle Bin toggles preserved.
- Gate: cleanup safety tests (blacklist, Recycle Bin routing) unchanged and passing.

## Task 6: Code health
- [x] Split the 830-line `MainPage.xaml.cs` into per-page view-models.
- [x] Split `crates/ffi/src/lib.rs` (about 2,500 lines) per feature module.
- Gate: tests identical before and after; no public FFI symbol renamed.

## Task 7: Performance review (findings first, changes second)
- [~] Profile scan on a large drive (Win32 tier done); MFT tier needs an elevated shell.
- [x] Look for: per-tile `GetNodeInfo` calls inside the Direct2D draw loop (called every frame), repeated `_rects` linear scans for hover/selection, layout recompute on every size change without debounce.
- [x] Report numbers before/after. Only keep changes that are measurably faster.
- Gate: benchmarks in README still met.

## Task 8: Release pipeline
- [x] Decide distribution: self-contained zip (recommended for "download and run") plus optional installer.
- [x] Fix `release.yml`: it currently publishes with `--self-contained false` and `PublishTrimmed=True`; WinUI 3 apps commonly break with trimming, and users without the .NET 9 Desktop Runtime and Windows App Runtime cannot launch the app.
- [x] Add ARM64 build, SHA-256 checksums (already present), release notes from CHANGELOG.
- [ ] Optional: code signing (unsigned downloads trigger SmartScreen warnings).
- [ ] Dry-run the workflow with a pre-release tag (`v0.2.0-rc1`) and install the produced zip on a clean machine.
- Gate: a fresh Windows 11 machine can download, unzip, and run it.

## Task 9: README and download flow
- [x] README "Download" section with the latest-release badge and a direct link (`/releases/latest`).
- [x] Release-asset naming that stays stable so `/releases/latest/download/<file>` works.
- [x] Landing page "Download" button points to the real release; show version and checksum.
- Gate: links resolve (200) after the first real release.

## Task 10: Discoverability (SEO, GEO, AEO)
- [x] Landing page: unique title/description, canonical, JSON-LD (SoftwareApplication + FAQPage), `sitemap.xml`, `robots.txt`.
- [~] Real `og-image.png` (1200x630) done; screenshots from the real app still to add; alt text.
- [x] `llms.txt` kept current; clear plain-language definitions ("what is X, who is it for, is it free") that AI answer engines can quote.
- [x] Comparison and how-to pages targeting: "free windows disk cleaner", "windows storage analyzer", "clean node_modules windows", "docker vhdx cleanup", "WinDirStat alternative".
- [ ] GitHub repo: topics, About text, social preview, pinned release, Discussions.
- [ ] Off-site: submit to winget, Scoop, Chocolatey, AlternativeTo, Product Hunt, Hacker News (Show HN), r/windows, r/rust, dev.to.
- Note: ranking #1 for generic terms like "windows cleaner" is not controllable and takes months; realistic goal is #1 for the product name and top results for long-tail developer queries.

## Open decisions
1. Final product name (SpaceOps vs rename). Blocks Tasks 8-10 branding, repo URL, package IDs.
2. Self-contained vs framework-dependent release (recommend self-contained).
3. Code-signing certificate (cost) vs shipping unsigned with documented SmartScreen steps.

## Status (integration/v0.2)
Done and verified locally (97 Rust + 34 C# tests, clean build, app launches without crashing):
- Tasks 1-5 (theme, sidebar shell, Overview, treemap polish, cleanup unification), C# code split, FFI module split, performance review (8 kept changes).
- Task 8 pipeline written; Task 9-10 README, landing page, guides, sitemap, robots, llms files, packaging drafts.

Not yet verified / still open:
- 60 FPS on a 1M-node tree (only a 16k-node folder was scanned in the UI tour).
- Treemap folders are always drawn in the "Other" grey; coloring folders by their dominant file category would make deep build trees readable (needs a Rust aggregate).
- Leftover detection is still a name-match heuristic (e.g. per-user folders in ProgramData, MSIX apps such as Claude get flagged); results are Medium risk and unselected, but a stronger signal (registry/MSIX package lookup) is needed before any default selection.
- App smoke check in CI (Task 0).
- Release workflow only validated locally (publish succeeds); arm64 cross-compile, release creation and notes extraction need a real run: push a `v0.2.0-rc1` tag or run the workflow with dry_run.
- Code signing, winget/Scoop/Chocolatey submissions, GitHub Pages source, repo topics and social preview (see docs/github-repo-settings.md and docs/launch-checklist.md).

UI tour (2026-10-17, read-only: typed a path, pressed Scan on the repo folder, switched pages; no Clean/Uninstall/Recycle pressed):
- All five pages render with real data; Overview, Treemap, Cleanup, Developer, Applications verified by screenshot.
- Fixed from the tour: folder headers in treemap, Depth selector wired (was a stub), `AppData\Local\Programs` and other shared folders no longer offered as leftovers, leftovers labelled Medium risk, registry sizes labelled "Registry est.", row text left-aligned.

Real-drive run (2026-10-17, read-only scan of a 2,164,935-node user profile; nothing cleaned):
- Scan 33-49 s (Win32 tier, not elevated), tree build ~0.5 s, page switches 35-314 ms, app stays responsive, no crash.
- Fixed: developer artifacts offered from installed software (app bundles, nvm, ~/.cargo/bin) and nested duplicates (7,896 -> 1,178); dormancy never worked because folders carry no mtime (0 -> 458 dormant).
- Fixed: cleanup analysis 22.2 s -> 2.0 s (Overview "Reclaimable now" waited ~26 s before).
- Fixed (feat/rust-quality): treemap folders colored by dominant content; leftovers skip folders active in the last 30 days (34 -> 15).
- Open: app working set ~620-750 MB at 2.16M nodes (the 55 MB figure is the core tree at 1M nodes only); worth a memory profile.
