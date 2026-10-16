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
- [ ] Add a `smoke` check: launch app, scan a temp folder, assert tree is non-empty.
- Gate: `cargo test --workspace` green AND `dotnet build -c Release` green AND integration tests green.

## Task 1: Theme foundation
- [ ] `Styles/Theme.xaml` resource dictionary: accent `#00F5D4`, blue `#3A86FF`, purple `#9D4EDD`, pink, amber; dark surfaces.
- [ ] Single source of truth for category colors (today duplicated in `TreemapService.cs` and the XAML legend).
- [ ] Replace inline hex values (`#FF9800`, `#10B981`, `#3B82F6`).
- Gate: app builds; visual diff of main screen.

## Task 2: Fluent Sidebar shell
- [ ] `NavigationView` (left, expanded) with: Overview, Treemap, Cleanup, Developer, Applications.
- [ ] Slim header: target path, Scan, Sync index. Bottom status bar kept.
- [ ] Convert the three modal overlays into pages (they currently hide the treemap).
- [ ] Drive-usage card at the bottom of the sidebar.
- Gate: every existing feature still reachable; no code path removed.

## Task 3: Overview page
- [ ] Usage donut by category, top 5 consumers, "Reclaimable now" total, quick actions.
- Gate: numbers match the scanned tree.

## Task 4: Treemap polish
- [ ] Rounded tiles, subtle gradient, folder vs file distinction, hover glow, details panel matching the mockup.
- Gate: layout hit-testing unchanged (parity tests); 60 FPS on a 1M-node tree.

## Task 5: Cleanup unification
- [ ] One candidate list across system junk, dev artifacts, and app leftovers; one reclaimable total; dry-run and Recycle Bin toggles preserved.
- Gate: cleanup safety tests (blacklist, Recycle Bin routing) unchanged and passing.

## Task 6: Code health
- [ ] Split the 830-line `MainPage.xaml.cs` into per-page view-models.
- [ ] Split `crates/ffi/src/lib.rs` (about 2,500 lines) per feature module.
- Gate: tests identical before and after; no public FFI symbol renamed.

## Task 7: Performance review (findings first, changes second)
- [ ] Profile scan on a large drive; compare MFT tier vs Win32 tier.
- [ ] Look for: per-tile `GetNodeInfo` calls inside the Direct2D draw loop (called every frame), repeated `_rects` linear scans for hover/selection, layout recompute on every size change without debounce.
- [ ] Report numbers before/after. Only keep changes that are measurably faster.
- Gate: benchmarks in README still met.

## Task 8: Release pipeline
- [ ] Decide distribution: self-contained zip (recommended for "download and run") plus optional installer.
- [ ] Fix `release.yml`: it currently publishes with `--self-contained false` and `PublishTrimmed=True`; WinUI 3 apps commonly break with trimming, and users without the .NET 9 Desktop Runtime and Windows App Runtime cannot launch the app.
- [ ] Add ARM64 build, SHA-256 checksums (already present), release notes from CHANGELOG.
- [ ] Optional: code signing (unsigned downloads trigger SmartScreen warnings).
- [ ] Dry-run the workflow with a pre-release tag (`v0.2.0-rc1`) and install the produced zip on a clean machine.
- Gate: a fresh Windows 11 machine can download, unzip, and run it.

## Task 9: README and download flow
- [ ] README "Download" section with the latest-release badge and a direct link (`/releases/latest`).
- [ ] Release-asset naming that stays stable so `/releases/latest/download/<file>` works.
- [ ] Landing page "Download" button points to the real release; show version and checksum.
- Gate: links resolve (200) after the first real release.

## Task 10: Discoverability (SEO, GEO, AEO)
- [ ] Landing page: unique title/description, canonical, JSON-LD (SoftwareApplication + FAQPage), `sitemap.xml`, `robots.txt`.
- [ ] Real `og-image.png` (1200x630) and screenshots from the real app; alt text.
- [ ] `llms.txt` kept current; clear plain-language definitions ("what is X, who is it for, is it free") that AI answer engines can quote.
- [ ] Comparison and how-to pages targeting: "free windows disk cleaner", "windows storage analyzer", "clean node_modules windows", "docker vhdx cleanup", "WinDirStat alternative".
- [ ] GitHub repo: topics, About text, social preview, pinned release, Discussions.
- [ ] Off-site: submit to winget, Scoop, Chocolatey, AlternativeTo, Product Hunt, Hacker News (Show HN), r/windows, r/rust, dev.to.
- Note: ranking #1 for generic terms like "windows cleaner" is not controllable and takes months; realistic goal is #1 for the product name and top results for long-tail developer queries.

## Open decisions
1. Final product name (SpaceOps vs rename). Blocks Tasks 8-10 branding, repo URL, package IDs.
2. Self-contained vs framework-dependent release (recommend self-contained).
3. Code-signing certificate (cost) vs shipping unsigned with documented SmartScreen steps.
