# Packaging drafts (fill after first release)

**Status: drafts only. Nothing here has been submitted anywhere.** Every value marked `FILL-AFTER-FIRST-RELEASE` (version, SHA-256 hashes, release date, the executable name inside the zip) must be replaced with real data from the first published GitHub release before submitting. See `docs/launch-checklist.md` for the submission steps.

| Folder | Target | Notes |
| :--- | :--- | :--- |
| `winget/` | Windows Package Manager community repo (`microsoft/winget-pkgs`) | Portable zip, x64 and ARM64 |
| `scoop/` | A Scoop bucket (own bucket first, `extras` later) | `spaceops.json` |
| `chocolatey/` | Chocolatey community repository | `.nuspec` plus install scripts |

Release asset names are fixed: `SpaceOps-win-x64.zip`, `SpaceOps-win-arm64.zip`, `SHA256SUMS.txt`. Download URL pattern for a tagged release: `https://github.com/ChadaSaiteja/SpaceOps/releases/download/v<VERSION>/<asset>`.

Compute a hash locally with `Get-FileHash .\SpaceOps-win-x64.zip -Algorithm SHA256`, or copy it from `SHA256SUMS.txt`.
