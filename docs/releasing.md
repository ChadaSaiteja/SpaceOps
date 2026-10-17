# Releasing SpaceOps

Pushing a version tag builds and publishes the downloads. Nothing else is manual.

## Cutting a release

1. Bump the version everywhere it lives:
   - the crate versions under `storage-intelligence/core/` (each crate's `Cargo.toml`; the workspace has no shared version yet)
   - `storage-intelligence/app/StorageIntelligence/StorageIntelligence.csproj` (add `<Version>` if not yet present)
   - `CHANGELOG.md`: add a `## [X.Y.Z] - YYYY-MM-DD` section. Its body becomes the release notes.
2. Merge to `main` once CI is green.
3. (Optional) Dry run: GitHub, Actions, Release, Run workflow, leave **dry run** checked. This builds both zips and attaches them to the workflow run without creating a release.
4. Tag and push:
   ```
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```
   A tag containing a hyphen (for example `v0.2.0-rc1`) is published as a pre-release automatically.

## What the workflow does (`.github/workflows/release.yml`)

- Builds `ffi.dll` with Rust for `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`.
- Runs `dotnet publish` with `-p:SpaceOpsRelease=true`, which turns on `SelfContained` and `WindowsAppSDKSelfContained` (unpackaged, no MSIX, no trimming, ReadyToRun kept). Regular `dotnet build` and CI are not affected.
- Copies `ffi.dll` next to `StorageIntelligence.exe`, zips, then verifies that the unzipped bundle contains the exe, `ffi.dll` and the .NET and Windows App SDK runtime files, and starts the x64 exe for a few seconds (non-blocking smoke test; the app has no `--version` flag).
- The win-arm64 job is cross-compiled on an x64 runner, cannot be smoke-tested, and is marked optional (`continue-on-error`). If it fails the release still ships win-x64 only.
- The release job (the only one with `contents: write`) writes `SHA256SUMS.txt`, takes release notes from the matching `CHANGELOG.md` section (falls back to GitHub generated notes), and creates the release.

## Stable asset names

| File | Stable URL |
| --- | --- |
| `SpaceOps-win-x64.zip` | `https://github.com/ChadaSaiteja/SpaceOps/releases/latest/download/SpaceOps-win-x64.zip` |
| `SpaceOps-win-arm64.zip` | `https://github.com/ChadaSaiteja/SpaceOps/releases/latest/download/SpaceOps-win-arm64.zip` |
| `SHA256SUMS.txt` | `https://github.com/ChadaSaiteja/SpaceOps/releases/latest/download/SHA256SUMS.txt` |

Do not rename these; the README and landing page link to them. Note that `/releases/latest` ignores pre-releases.

## Verifying a download (users)

PowerShell, in the folder containing the zip and `SHA256SUMS.txt`:

```powershell
(Get-FileHash .\SpaceOps-win-x64.zip -Algorithm SHA256).Hash.ToLower()
Select-String SpaceOps-win-x64.zip .\SHA256SUMS.txt
```

The two hashes must match. Then extract the zip and run `StorageIntelligence.exe`; no .NET or Windows App Runtime install is needed.

## SmartScreen (unsigned builds)

Builds are not code-signed yet, so Windows SmartScreen may show "Windows protected your PC". Choose **More info**, then **Run anyway**. If the zip was downloaded in a browser you can also right-click it, Properties, tick **Unblock**, before extracting. Verify the SHA-256 first.

## Adding code signing later

Obtain a certificate (an OV/EV certificate or Azure Trusted Signing), store it as repository secrets, and add a step in the `build` job after the "Copy native DLL" step and before "Create zip". Placeholder:

```yaml
# - name: Sign binaries
#   env:
#     CERT_PFX_BASE64: ${{ secrets.CODESIGN_PFX_BASE64 }}
#     CERT_PASSWORD: ${{ secrets.CODESIGN_PFX_PASSWORD }}
#   run: |
#     [IO.File]::WriteAllBytes("cert.pfx", [Convert]::FromBase64String($env:CERT_PFX_BASE64))
#     $signtool = (Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName | Select-Object -Last 1).FullName
#     & $signtool sign /f cert.pfx /p $env:CERT_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 `
#       ./publish/StorageIntelligence.exe ./publish/ffi.dll
#     Remove-Item cert.pfx
```

Signing before zipping means the hashes in `SHA256SUMS.txt` cover the signed files. SmartScreen reputation still builds over time for OV certificates.
