# GitHub repository settings (kept as a file)

These settings live in the GitHub web UI, not in git, so this file records the exact values to apply (Settings and the repo home page gear icon next to "About"). Nothing here is applied automatically.

## About

**Description** (350 characters max):

```text
Free, open-source disk analyzer and storage cleaner for Windows 10/11. Rust scanner, Direct2D treemap, developer-cache detection (node_modules, Cargo target, WSL/Docker VHDX), Recycle Bin-safe cleanup. 100% offline, MIT.
```

**Website**: `https://chadasaiteja.github.io/SpaceOps/`

## Topics (exactly 20)

```text
windows windows-10 windows-11 disk-space disk-analyzer storage-analyzer disk-cleaner treemap windirstat-alternative rust winui3 dotnet developer-tools ntfs usn-journal wsl2 docker node-modules sqlite open-source
```

Using the GitHub CLI (run by the repo owner):

```bash
gh repo edit ChadaSaiteja/SpaceOps \
  --description "Free, open-source disk analyzer and storage cleaner for Windows 10/11. Rust scanner, Direct2D treemap, developer-cache detection (node_modules, Cargo target, WSL/Docker VHDX), Recycle Bin-safe cleanup. 100% offline, MIT." \
  --homepage "https://chadasaiteja.github.io/SpaceOps/" \
  --add-topic windows,windows-10,windows-11,disk-space,disk-analyzer,storage-analyzer,disk-cleaner,treemap,windirstat-alternative,rust,winui3,dotnet,developer-tools,ntfs,usn-journal,wsl2,docker,node-modules,sqlite,open-source
```

## Social preview image

1. Settings > General > Social preview > **Edit** > Upload an image.
2. Upload `og-image.png` from the repository root (1200x630 px, under 1 MB; GitHub recommends 1280x640 and accepts other sizes of at least 640x320, so re-export at 1280x640 if the preview looks cropped).
3. Verify by pasting the repo URL into a Slack or Discord message, or at the [Open Graph debugger](https://www.opengraph.xyz/).

## GitHub Pages

Settings > Pages > Build and deployment > Source: **GitHub Actions** (the `Deploy Pages` workflow in `.github/workflows/pages.yml` publishes `index.html`, `docs-site/`, `sitemap.xml`, `robots.txt`, `llms.txt`, icons and the OG image to the Pages root).

Note: crawlers only honor `robots.txt` at the host root (`https://chadasaiteja.github.io/robots.txt`), which a project site cannot control. The `sitemap.xml` is still referenced by `robots.txt` and can be submitted directly in Google Search Console and Bing Webmaster Tools.

## Releases

- Pin the first stable release: Releases > the release > **Set as the latest release**.
- Release assets must keep these exact names so the direct links in the README and site keep working: `SpaceOps-win-x64.zip`, `SpaceOps-win-arm64.zip`, `SHA256SUMS.txt`.
- Direct links: `https://github.com/ChadaSaiteja/SpaceOps/releases/latest/download/<asset>`.

## Discussions

1. Settings > General > Features > tick **Discussions**.
2. Discussions > pencil icon (Edit categories). Suggested categories:
   - **Announcements** (maintainers only, announcement format)
   - **Q&A** (question/answer format)
   - **Ideas** (open-ended)
   - **Show and tell** (open-ended)
   - **General** (open-ended)
3. Pin a welcome post: "Welcome to SpaceOps: ask questions, share what you reclaimed, propose features."
4. After enabling, update the "Ask Questions" bullet in `README.md` and `SUPPORT.md` (they currently say Discussions is not enabled).

## Other recommended settings

- Settings > General > Features: keep Issues on; Wikis and Projects off unless used.
- Settings > Code security: enable **Private vulnerability reporting** (matches `SECURITY.md`), Dependabot alerts and Dependabot security updates.
- Settings > Branches: protect `main` (require PR and passing CI).
- Pin the repository on the owner's profile.
