# Launch checklist and off-site distribution plan

**Everything below is marked "fill after first release". Nothing has been submitted anywhere.** Do these steps in order, only after a real GitHub release exists. Replace `<VERSION>` with the real version, and never invent numbers: quote only figures that are in the README or measured by you.

Personal anecdotes in the sample copy below are templates: edit them to match your real story before posting.

Fixed release asset names: `SpaceOps-win-x64.zip`, `SpaceOps-win-arm64.zip`, `SHA256SUMS.txt`.

## 0. Pre-flight (fill after first release)

- [ ] Release is published (not draft) and marked **Latest**; the three assets above are attached.
- [ ] These URLs return HTTP 200 (check in a browser or `curl.exe -I -L <url>`):
  - `https://github.com/ChadaSaiteja/SpaceOps/releases/latest`
  - `https://github.com/ChadaSaiteja/SpaceOps/releases/latest/download/SpaceOps-win-x64.zip`
  - `https://github.com/ChadaSaiteja/SpaceOps/releases/latest/download/SpaceOps-win-arm64.zip`
  - `https://github.com/ChadaSaiteja/SpaceOps/releases/latest/download/SHA256SUMS.txt`
- [ ] Downloaded the x64 zip on a clean Windows 10/11 machine, verified the SHA-256, ran it. Note the real executable name inside the zip (the packaging drafts assume it is unknown).
- [ ] GitHub Pages is deployed (Actions > Deploy Pages is green) and `https://chadasaiteja.github.io/SpaceOps/sitemap.xml` loads.
- [ ] Repo About, topics and social preview applied per `docs/github-repo-settings.md`; Discussions enabled.
- [ ] Take **real screenshots** of the running app (light and dark). The landing page mockup is illustrative; use real screenshots for any off-site listing. Save as PNG, 1920x1080 or similar.
- [ ] Submit `https://chadasaiteja.github.io/SpaceOps/sitemap.xml` in Google Search Console and Bing Webmaster Tools (verify the site with the HTML-tag method on `index.html` if required).
- [ ] Update `softwareVersion`/`datePublished` in `index.html` JSON-LD, `version`/`date-released` in `CITATION.cff` and `codemeta.json`, and the `lastmod` dates in `sitemap.xml`.

Reality check: ranking first for generic terms such as "windows cleaner" is not controllable and takes months. Realistic goals are first for the product name and strong results for long-tail developer queries.

## 1. winget (Windows Package Manager)

Draft manifests: `packaging/winget/manifests/s/SaitejaChada/SpaceOps/FILL-AFTER-FIRST-RELEASE/` (three YAML files).

- [ ] Rename the folder to the version (for example `0.2.0`) and fill `PackageVersion`, installer URLs, `InstallerSha256` (from `SHA256SUMS.txt`, uppercase), `ReleaseDate`, and the nested `RelativeFilePath` (the exe inside the zip).
- [ ] Validate locally: `winget validate --manifest <folder>` and `winget install --manifest <folder>` (needs `winget settings --enable LocalManifestFiles`).
- [ ] Fork `microsoft/winget-pkgs`, copy the folder to `manifests/s/SaitejaChada/SpaceOps/<VERSION>/`, open a PR. Or use `wingetcreate new <x64 zip URL>` to generate and submit.
- [ ] Expect Microsoft's automated validation, including a SmartScreen/antivirus scan; an unsigned build may need a manual review or may be rejected until signed.

PR title (ready to paste): `New package: SaitejaChada.SpaceOps version <VERSION>`

## 2. Scoop

Draft: `packaging/scoop/spaceops.json`.

- [ ] Fill `version`, both `hash` values, `bin` and `shortcuts` (the exe name inside the zip).
- [ ] Test: `scoop install .\spaceops.json`, then `scoop uninstall spaceops`.
- [ ] Start with your own bucket (a GitHub repo `scoop-bucket` containing `bucket/spaceops.json`); users run `scoop bucket add spaceops https://github.com/ChadaSaiteja/scoop-bucket` then `scoop install spaceops`. Consider a PR to the `extras` bucket once the project has some traction (extras has notability criteria; check its README first).

## 3. Chocolatey

Draft: `packaging/chocolatey/` (nuspec plus `tools/chocolateyinstall.ps1`).

- [ ] Fill the version, checksum and release-notes URL; add `tools/` only the scripts (the zip is downloaded, not embedded).
- [ ] `choco pack`, then test with `choco install spaceops -s . -y` in a clean VM or sandbox.
- [ ] Create an account on community.chocolatey.org, get an API key, `choco push` (moderation takes days and may ask for fixes; unsigned binaries and a missing icon or docs are common review comments).

## 4. AlternativeTo

- [ ] Add the app at alternativeto.net (requires an account): name `SpaceOps`, website `https://chadasaiteja.github.io/SpaceOps/`, platform Windows, license Open Source (MIT), price Free.
- [ ] Mark it as an alternative to WinDirStat, WizTree and TreeSize (only list tools it genuinely replaces for you).

Copy (fill after first release):

- Tagline: `Free, open-source disk analyzer for Windows with a treemap and developer-cache detection`
- Description: `SpaceOps scans your drives with a parallel Rust engine and shows disk usage as an interactive treemap. It detects developer clutter such as node_modules, Rust target folders, Python venvs and WSL/Docker virtual disks, and sends every cleanup to the Recycle Bin, with a dry run first. MIT licensed, 100% offline, Windows 10/11 on x64 and ARM64. Early v0.x; builds are unsigned.`

## 5. Product Hunt

- [ ] Launch on a Tuesday to Thursday; prepare a gallery of real screenshots and a short demo GIF or video (real app).
- [ ] Add a maker comment (below) and reply to every comment on launch day.

Copy (fill after first release):

- Name: `SpaceOps`
- Tagline (60 chars max): `Find what eats your Windows disk. Clean it with an undo.`
- Description (260 chars max): `Free, open-source disk analyzer for Windows. Treemap view, developer-clutter detection (node_modules, Cargo target, WSL/Docker disks) and Recycle Bin-safe cleanup. 100% offline, MIT.`
- Maker comment:

```text
Hi, I'm Saiteja, the maker of SpaceOps. My dev disk kept filling up with node_modules, Cargo target folders and WSL virtual disks, and the usual tools did not tell me which of those were safe to remove. SpaceOps scans with a Rust engine, shows a treemap, flags dormant projects, and sends every cleanup to the Recycle Bin after a dry run. It is free, MIT licensed and fully offline. It is early (v<VERSION>), Windows-only and the builds are unsigned for now, so please verify the checksum. I would love feedback on what is missing.
```

## 6. Show HN (Hacker News)

- [ ] Title must be factual, no marketing language. Post at a weekday morning US time. Link to the GitHub repo (HN prefers source). Add a first comment immediately.

Copy (fill after first release):

- Title: `Show HN: SpaceOps – open-source Windows disk analyzer with developer-cache detection`
- URL: `https://github.com/ChadaSaiteja/SpaceOps`
- First comment:

```text
I built SpaceOps because my Windows dev machine kept running out of space and the cause was usually node_modules, Cargo target dirs and WSL2/Docker VHDX files that don't shrink on their own. It scans with a parallel Rust core, renders a squarified treemap (WinUI 3 + Direct2D), flags projects untouched for 30+ days, and routes deletions through the Recycle Bin with a dry run and a hard blacklist for system paths. Fully offline, MIT.
Limitations: Windows only, v<VERSION>, builds are unsigned (SmartScreen will warn; checksums are published). The README has a 1M-node synthetic benchmark you can reproduce with cargo. Feedback on safety rules and missing ecosystems especially welcome.
```

## 7. Reddit (read each subreddit's rules first; many limit self-promotion)

### r/rust (use the "project" flair if offered)

- Title: `SpaceOps: a Windows disk analyzer with a Rust scanning core (Rayon, squarified treemap, USN journal sync)`
- Body:

```text
I've been building SpaceOps, an open-source (MIT) disk analyzer for Windows. The core is a Rust workspace: a Rayon-based Win32 scanner reporting cluster-allocated sizes, an arena-based storage tree, a squarified treemap layout, SQLite (WAL) persistence and NTFS USN journal incremental sync, exposed over a C ABI (csbindgen) to a WinUI 3 front end. Happy to talk about the FFI boundary, the arena design and the benchmark (cargo run --release -p storage-tree --example tree_benchmark). Repo: https://github.com/ChadaSaiteja/SpaceOps. Early v<VERSION>, feedback welcome.
```

### r/windows (check whether project posts are allowed; otherwise skip)

- Title: `I made a free, open-source disk space analyzer for Windows (treemap, Recycle Bin-safe cleanup)`
- Body: short description, link to the site and release, mention it is unsigned (checksum provided), v0.x and what feedback you want.

### Optional: r/sysadmin, r/webdev, r/node, r/docker only if their rules allow, with angle-specific posts (for example the WSL2 VHDX guide).

## 8. dev.to post outline

Title: `Why your Windows dev disk is full (node_modules, Cargo target, WSL2 VHDX) and how to reclaim it safely`

1. The problem: a short, honest story of a full disk (use your own real numbers, do not invent).
2. The usual culprits: node_modules, Cargo `target/`, WSL2/Docker `ext4.vhdx` (do not auto-shrink), .NET `bin/obj`, Python venvs.
3. How to reclaim each manually (commands from the site guides: `cargo clean`, `wsl --shutdown` + `diskpart compact vdisk`).
4. Why a treemap helps: seeing sizes at a glance.
5. Introducing SpaceOps: what it does, safety model (blacklist, dry run, Recycle Bin), limitations (Windows only, v0.x, unsigned).
6. Download, checksum verification, link to repo and site guides.
7. Call for feedback.

Tags: `windows`, `rust`, `devtools`, `opensource`. Add a cover image from `og-image.png` or a real screenshot. Set the canonical URL to the matching site guide if you cross-post.

## 9. After launch

- [ ] Reply to every issue and discussion within a day; label `good first issue` items.
- [ ] Add the winget, Scoop and Chocolatey install commands to the README and site only once each is actually live.
- [ ] Track what worked with GitHub traffic insights (Insights > Traffic); do not add analytics scripts to the site (the project promises zero telemetry).
