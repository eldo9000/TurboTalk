# Releasing TurboTalk

> **This beta release is unsigned on all platforms.** The macOS DMG uses ad-hoc signing only (`signingIdentity: "-"` in `tauri.conf.json`); the Windows `.exe` and the Linux AppImage have **no signature at all**. Users will see Gatekeeper warnings on macOS, SmartScreen warnings on Windows, and no signature check on Linux. Do not run any code-signing or notarization step for this beta. The Developer ID + notarization scaffolding is present in this repo and `BUILD.md` for a future signed release — see [Future signed releases (deferred)](#future-signed-releases-deferred) below — but it is **not** executed for this version.

## Scope

This is the procedure for cutting a TurboTalk beta release. Three platforms:

- **macOS arm64** (Apple Silicon) — the primary, currently-shipping target.
- **Windows x64** — beta in progress; build procedure is documented but artifacts cannot be produced today on a Windows host because the bundled Whisper sidecar binary for Windows is not yet in the repo (pending TASK-27).
- **Linux x64 (X11)** — beta in progress; same gap as Windows.

This document is the *release procedure* (versioning, tagging, publishing, release notes); `BUILD.md` is the *build procedure* (compiling, packaging). Follow the pre-flight, then the per-platform build sections, then tag/publish.

## Pre-flight

Before doing anything else, confirm:

- `git status` is clean and you are on `main`.
- `cargo test` passes (run from `src-tauri/`).
- `cargo clippy -- -D warnings` is green (run from `src-tauri/`).
- A manual smoke test on a fresh `npm run tauri dev` build passes — at minimum, complete Test 3 in `SMOKE-TEST.md` (push-to-talk → transcript pastes into the focused editor).

If any of the above fails, stop. Do not cut a release on top of a red tree.

## Step 1 — Bump the version

```bash
npm run bump-version -- <new-version>
```

This script updates `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` together. Inspect the diff to confirm all three files moved to the same version, then commit:

```bash
git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json Cargo.lock
git commit -m "chore(release): bump to <new-version>"
```

## Step 2 — Build artifacts on each host

Run the per-platform build section that matches your build host. Each section below produces an unsigned artifact and its `.sha256` companion file in `dist-artifacts/`.

### Build procedure — macOS

Build host: macOS on Apple Silicon.

```bash
npm install
npm run package
```

For this beta, do **not** set the `APPLE_*` environment variables described in `BUILD.md`. With those env vars unset, `tauri build` produces an ad-hoc-signed DMG (no notarization step, no upload). The build typically completes in 1–3 minutes.

When it finishes, confirm both files exist:

```
dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg
dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg.sha256
```

Verify ad-hoc signing with:

```bash
codesign -dv dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg
```

The `Authority` line should be absent or read `Authority=-` (ad-hoc). A `Notarized Developer ID` source means somebody set the `APPLE_*` env vars — back out and rebuild without them.

### Build procedure — Windows

Build host: Windows 10 (1809+) or Windows 11, x64.

> **Prerequisite gap (current):** the bundled Whisper sidecar binary for Windows (`src-tauri/binaries/whisper-cli-x86_64-pc-windows-msvc.exe`) is not yet in the repo. Until that binary is in place (pending TASK-27), `npm run package` on Windows will fail at the bundler stage. The procedure below is correct in shape and will work once the binary lands.

Required toolchain (one-time setup on the build host):

- Rust toolchain (rustup) with the `x86_64-pc-windows-msvc` target.
- Visual Studio Build Tools 2022 with the "Desktop development with C++" workload (provides `cl.exe`, the MSVC linker, and the Windows SDK).
- Node.js 20+.
- WebView2 SDK (usually picked up automatically; included with the VS Build Tools workload).

Once `src-tauri/binaries/whisper-cli-x86_64-pc-windows-msvc.exe` is in place:

```powershell
npm install
npm run package
```

Expected artifacts:

```
dist-artifacts/TurboTalk-<new-version>-windows-x64-setup.exe
dist-artifacts/TurboTalk-<new-version>-windows-x64-setup.exe.sha256
```

The `.exe` is an NSIS installer. **It is unsigned.** End users will see SmartScreen "Windows protected your PC" on first run — that is documented in `README.md`. Do **not** sign the installer for this beta.

### Build procedure — Linux (X11)

Build host: Ubuntu 22.04 or any Debian-derivative with the Tauri 2 prereqs installed. Equivalent packages exist on Fedora/Arch — see <https://tauri.app/start/prerequisites/>.

> **Prerequisite gap (current):** the bundled Whisper sidecar binary for Linux (`src-tauri/binaries/whisper-cli-x86_64-unknown-linux-gnu`) is not yet in the repo. Until that binary is in place (pending TASK-27), `npm run package` on Linux will fail at the bundler stage. The procedure below is correct in shape and will work once the binary lands.

Required system packages (one-time setup):

```bash
sudo apt update
sudo apt install -y \
    libwebkit2gtk-4.1-dev \
    build-essential \
    curl \
    wget \
    file \
    libxdo-dev \
    libssl-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libfuse2
```

Plus Rust toolchain (rustup) and Node.js 20+.

Once `src-tauri/binaries/whisper-cli-x86_64-unknown-linux-gnu` is in place:

```bash
npm install
npm run package
```

Expected artifacts:

```
dist-artifacts/TurboTalk-<new-version>-linux-x64.AppImage
dist-artifacts/TurboTalk-<new-version>-linux-x64.AppImage.sha256
```

The AppImage is **unsigned**. Users `chmod +x` and run it directly. There is no `.deb` or `.rpm` for this beta — one AppImage covers all distros that have FUSE installed.

## Step 3 — Verify artifacts

For each artifact you intend to publish, verify the matching `.sha256` and run the **per-platform installed-artifact smoke test**:

- macOS: `SMOKE-TEST.md` → "macOS beta smoke test" + the 11-step "Installed-artifact smoke test (macOS)" subsection.
- Windows: `SMOKE-TEST.md` → "Windows beta smoke test" (the 7 W-tests).
- Linux: `SMOKE-TEST.md` → "Linux beta smoke test (X11)" (the 7 L-tests).

Every numbered step in the smoke test for each platform you are publishing must pass before tagging.

## Step 4 — Tag and publish

```bash
git tag v<new-version>
git push origin main --tags
```

Write the release notes for this version into `RELEASE_NOTES.md` at the repo root using the template at the bottom of this file, then publish.

The exact `gh release create` invocation depends on which platforms you are publishing this version. For a multi-platform release, attach all six files (3 artifacts × {artifact, .sha256}):

```bash
gh release create v<new-version> \
  --title "TurboTalk <new-version> beta" \
  --notes-file RELEASE_NOTES.md \
  dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg \
  dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg.sha256 \
  dist-artifacts/TurboTalk-<new-version>-windows-x64-setup.exe \
  dist-artifacts/TurboTalk-<new-version>-windows-x64-setup.exe.sha256 \
  dist-artifacts/TurboTalk-<new-version>-linux-x64.AppImage \
  dist-artifacts/TurboTalk-<new-version>-linux-x64.AppImage.sha256
```

For a macOS-only release (the current state), attach only the two macOS files. Drop the rest of the lines.

`RELEASE_NOTES.md` is a scratch file per release — do not commit it.

## Step 5 — Update SESSION-STATUS.md

Add a single line under "Where We Are":

> Released v<new-version> beta on <YYYY-MM-DD>.

Commit as `chore(status): record v<new-version> release`.

## Manual updates policy

**TurboTalk beta uses manual updates only.** When a new release ships, users download the new artifact and replace the old one. The Tauri updater plugin is intentionally **not** enabled.

Before enabling auto-update we need: (1) a long-lived updater signing key with a documented secure-custody plan, (2) a stable artifact-hosting URL that we control, (3) a written key-rotation/loss procedure. See `BETA-AUDIT-ROADMAP.md` line 333 — "Do not add auto-update until artifact naming, signing, and release hosting are stable." Until those three exist, do not enable the updater plugin.

## Future signed releases (deferred)

> **Not used for this beta.** The commands and env vars below are kept here so the procedure is not lost when we eventually do sign a release. Do not run any of them for the current beta.

When we eventually publish a signed and notarized macOS DMG, the procedure is:

1. Set the four `APPLE_*` environment variables described in `BUILD.md` → "Release build (signed + notarized)" → "Required env vars" (Apple ID, app-specific password, team ID, signing identity).
2. Run `npm run package` from a Mac with the Developer ID Application certificate installed in the login keychain.
3. The notarization upload + scan typically takes 5–15 minutes; the build will appear to hang during that window.
4. Verify with `spctl -a -t open --context context:primary-signature -v <dmg>` — expect `accepted` and `source=Notarized Developer ID`.

Future Windows signing will require an EV or OV code-signing certificate and `signtool`. Future Linux signing for an AppImage typically uses GPG-signed `.zsync` + a detached `.sig` — neither is set up.

None of this is in scope for the current beta. The `tauri.conf.json` `signingIdentity` is `"-"` (ad-hoc) deliberately.

## Release notes template

Copy this into `RELEASE_NOTES.md` and fill in the `<placeholders>`. For a single-platform release, drop the rows for platforms you are not publishing.

```markdown
## TurboTalk <version> beta

**Platforms in this release:**
- macOS 12+ on Apple Silicon (arm64).
- Windows 10 (1809+) / 11 on x64.
- Linux x64 on X11 sessions only.

**Install — macOS:** Download `TurboTalk-<version>-macos-arm64.dmg`, drag `Turbo Talk.app` into `/Applications`, **right-click → Open** on first launch (the DMG is ad-hoc signed, not notarized), and grant Microphone and Accessibility permissions when prompted.

**Install — Windows:** Download `TurboTalk-<version>-windows-x64-setup.exe`. SmartScreen will warn that the installer is unsigned — click **More info → Run anyway**. Run the installer and launch from the Start menu. WebView2 runtime is required (preinstalled on Windows 11; Windows 10 users may need <https://developer.microsoft.com/microsoft-edge/webview2/>).

**Install — Linux:** Download `TurboTalk-<version>-linux-x64.AppImage`, `chmod +x`, and run. Requires FUSE (`libfuse2` on Debian/Ubuntu) and an X11 session — Wayland is not supported.

**What's in this release**
- <one bullet per user-visible change>

**Known limitations**
- macOS: Apple Silicon only; ad-hoc signed (Gatekeeper warning on first launch).
- Windows: unsigned installer (SmartScreen warning on first run).
- Linux: X11 only; AppImage requires FUSE.
- <other known issues>

**Privacy and data** — see `PRIVACY.md`. To delete all local TurboTalk data, follow `PRIVACY.md` → "How to delete everything".

**Verify the download**

    # macOS / Linux:
    shasum -a 256 -c TurboTalk-<version>-macos-arm64.dmg.sha256
    shasum -a 256 -c TurboTalk-<version>-linux-x64.AppImage.sha256

    # Windows (PowerShell):
    Get-FileHash -Algorithm SHA256 TurboTalk-<version>-windows-x64-setup.exe

Compare the Windows hash output against the contents of `TurboTalk-<version>-windows-x64-setup.exe.sha256`.

**Feedback** — file an issue at https://github.com/eldo9000/TurboTalk-App/issues
```

## Hotfix path

Beta hotfixes follow the same procedure: bump the patch version, rebuild on the relevant host(s), re-verify, re-publish. There is no separate fast-path.

## Rollback

We do not delete published GitHub releases. If a release is broken, mark it as **pre-release** (`gh release edit v<version> --prerelease`) so the "Latest" badge moves off it, then publish a corrected release with a bumped patch version. The broken release stays visible for forensics.
