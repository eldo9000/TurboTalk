# TASK-31: GitHub Actions CI matrix for Win/Linux/Mac unsigned beta builds

## Goal
A GitHub Actions workflow at `.github/workflows/release.yml` builds unsigned beta artifacts on `macos-latest` (arm64), `windows-latest`, and `ubuntu-latest` runners, attaches them to a GitHub release on tag push (`v*`), and uploads matching `.sha256` files. Local hand-builds remain supported but are no longer the only path.

## Context
This is the **last** of the Win/Linux beta sprint tasks. It only matters once the codebase actually compiles + builds on each target — i.e., TASK-24 through TASK-28 must be landed first. Treat this as **optional** per the original scope; defer if any of the upstream tasks slipped.

There is **no existing CI workflow** in this repo (the v0.8 mac beta was hand-built). Per `CLAUDE.md`: "Personal-use tool — no CI gates required initially. Add CI when first usable build lands." The first usable Win/Linux build is exactly that point.

The workflow runs on:
- Tag push (`v*`) — full matrix build, attach artifacts to release.
- Manual `workflow_dispatch` — for testing the workflow itself without cutting a release.

Per-runner setup:

**`macos-latest`** (arm64 by default in 2026):
- Install Node, Rust, Tauri prereqs (`xcode-select` already present).
- Run the existing `npm run package`.
- Output: `TurboTalk-<v>-macos-arm64.dmg` + `.sha256`.

**`windows-latest`**:
- Install Node, Rust (`rustup-init.exe`).
- Tauri prereqs on Windows are minimal — WebView2 SDK comes via cargo build for Tauri.
- Run `npm run fetch-sidecars` (from TASK-27) to pull the Win whisper-cli + DLLs.
- Run `npm run package` → `dist-artifacts/TurboTalk-<v>-windows-x64-setup.exe` + `.sha256`.

**`ubuntu-latest`** (22.04 currently, transitioning to 24.04):
- Install Node, Rust.
- Apt install Tauri 2 Linux prereqs: `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libfuse2` (for AppImage). Reference: `https://v2.tauri.app/start/prerequisites/#linux`.
- Run `npm run fetch-sidecars` for Linux binaries.
- Run `npm run package` → `dist-artifacts/TurboTalk-<v>-linux-x64.AppImage` + `.sha256`.

Each job uploads its artifact + `.sha256` as a workflow artifact. A final `release` job (depends on all three) downloads the artifacts and runs `gh release create v<x.y.z> dist-artifacts/* --draft --notes-from-tag` (or pushes to an existing draft release if one matches the tag).

**No code signing.** The workflow must NOT reference Apple Developer ID env vars, NOT reference a Windows `.pfx`, NOT reference Linux GPG keys. The artifact names and hashes are the proof; users will see SmartScreen warnings on Win and that's documented in TASK-30's README.

Cache rust target/ between builds via `Swatinem/rust-cache@v2` keyed on platform + lockfile.

This task only writes the workflow. It does **not** debug the workflow on the actual GH runners — that's a separate iteration loop (push tag, watch run, fix). Reasonable to expect 1–2 follow-up commits to fix path issues, missing system packages, etc., once the workflow runs for real.

## In scope
- `.github/workflows/release.yml` — new file
- `.github/workflows/build.yml` (optional, for PRs) — only if the user explicitly wants per-PR builds; default is no
- `.github/dependabot.yml` if not already present — optional, low-priority

## Out of scope
- All upstream tasks (TASK-24 through TASK-30)
- Code signing of any kind
- Automated changelog generation
- Release-notes templating beyond `--notes-from-tag`
- Slack/Discord/email notifications
- Caching beyond `rust-cache`
- Any GH Pages, docs publishing, or website deploy
- macOS Intel (x86_64) builds — arm64 only for this beta
- Linux .deb packaging — AppImage only

## Steps
1. Confirm TASK-24 through TASK-28 are landed (sidecar lookup, hotkey, paste, sidecar binaries, build scripts). If any are pending, stop and tell the user.
2. Create `.github/workflows/release.yml`. Top-level keys:
   - `name: release`
   - `on: push: tags: ['v*']` and `workflow_dispatch:`
3. Define a `build` job using `strategy.matrix` with three entries: `os: macos-latest`, `windows-latest`, `ubuntu-latest`. Each entry sets `target_label: macos-arm64 | windows-x64 | linux-x64`.
4. Per-step:
   - `actions/checkout@v4`
   - `actions/setup-node@v4` with `node-version: 20` and `cache: 'npm'`
   - Install Rust via `dtolnay/rust-toolchain@stable`
   - `Swatinem/rust-cache@v2` with `workspaces: src-tauri`
   - On Linux only: an `apt-get install` step with the Tauri prereqs list above. Use `runs-on == 'ubuntu-latest'` conditional.
   - `npm install`
   - `npm run fetch-sidecars` (this script must exist from TASK-27; it should be a no-op on macOS where the sidecar is committed)
   - `npm run package`
   - `actions/upload-artifact@v4` with `name: turbotalk-${{ matrix.target_label }}`, `path: dist-artifacts/*`
5. Define a `release` job that `needs: build` and only runs on tag push (not on `workflow_dispatch`):
   - `actions/checkout@v4` (for the tag)
   - `actions/download-artifact@v4` with `path: dist-artifacts`, no `name` filter (downloads all)
   - `gh release create ${{ github.ref_name }} dist-artifacts/**/* --draft --title ${{ github.ref_name }} --notes-from-tag`. Run `gh` from `runs-on: ubuntu-latest` and pass `GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}`.
6. Validate the YAML syntax: `actionlint` if installed locally, otherwise rely on GH's syntax check on first push.
7. Manual smoke: trigger via `workflow_dispatch` once on a feature branch (no tag) to confirm the build job succeeds on all three runners. The release job is gated to tag push so it won't fire on dispatch.

## Success signal
- `.github/workflows/release.yml` exists and passes `gh workflow view release` (or shows up under Actions tab in the GH UI).
- A `workflow_dispatch` run completes all three matrix legs green and uploads three artifacts (one per target).
- A test tag push (e.g. `v0.0.0-ci-test`) produces a draft GitHub release with three `.exe`/`.dmg`/`.AppImage` files plus three `.sha256` files attached.
- `grep -i "signing\|notari\|certificate\|\.pfx\|developerid" .github/workflows/release.yml` returns nothing (workflow is unsigned end to end).

## Notes
- GH Actions runner OS images change. If the workflow breaks 6 months from now because `ubuntu-22.04` was removed, pin a specific image (`ubuntu-22.04`) explicitly.
- WebKitGTK package name has flipped between `4.0-dev` and `4.1-dev` across distros. `4.1-dev` is correct for Tauri 2.
- `gh release create --draft` keeps the release private until manually published — safer for first attempts.
- Don't run tests in this workflow. Tests run locally / in a separate PR workflow if added later.
- If `fetch-sidecars` ends up downloading from a private repo, you'll need a PAT with read access stored as a repo secret. Document that in the workflow's top comment.

→ verify: a tag push of `v0.9.0-beta1` (or whatever the next bump is) produces three downloadable artifacts on the GH releases page, each with a working .sha256 companion.
