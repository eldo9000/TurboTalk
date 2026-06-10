# Releasing TurboTalk

> **This beta release is unsigned/not notarized.** GitHub-downloaded macOS artifacts carry Apple's download quarantine flag, so users will see Gatekeeper warnings and may need right-click → Open or System Settings → Privacy & Security → Open Anyway. The Windows `.exe` has no Authenticode signature and will trigger SmartScreen. Do not run any code-signing or notarization step for this beta. The Developer ID + notarization scaffolding is present in this repo and `BUILD.md` for a future signed release — see [Future signed releases (deferred)](#future-signed-releases-deferred) below — but it is **not** executed for this version.

## Scope

This is the procedure for cutting a TurboTalk beta release.

- **macOS arm64** (Apple Silicon) — the primary, currently-shipping target.
- **Windows x64** — packaging can be produced in CI, including the Whisper sidecar, but the runtime dictation loop is not release-ready because Windows hotkey + paste are still unsupported stubs (TASK-25/26).
- **Linux x64 (X11)** — excluded from the release matrix until Linux sidecar, hotkey, and paste are validated on real hardware.

This document is the *release procedure* (versioning, tagging, publishing, release notes); `BUILD.md` is the *build procedure* (compiling, packaging). Follow the pre-flight, then the per-platform build sections, then tag/publish.

## Pre-flight

Before doing anything else, confirm:

- `git status` is clean and you are on `main`.
- `cargo test` passes (run from `src-tauri/`).
- `cargo clippy -- -D warnings` is green (run from `src-tauri/`).
- A manual smoke test on a fresh `npm run tauri dev` build passes — at minimum, complete Test 3 in `SMOKE-TEST.md` (push-to-talk → transcript pastes into the focused editor).
- Preflight integrity checks pass: `npm run preflight` verifies that all
  committed macOS native binaries match their pinned SHA-256 digests in
  `src-tauri/binaries/MANIFEST.sha256`. On Windows, `npm run fetch-sidecars`
  and `npm run fetch-onnxruntime` each verify their downloaded archives
  against pinned hashes before extracting.
- Runtime Whisper `.bin`, Moonshine ONNX, and Parakeet ONNX model downloads
  are verified against pinned SHA-256 hashes before being persisted to disk.
- The beta release scan pack in `RELEASE-READINESS.md` has no undocumented blockers:
  version consistency, updater/manual-update consistency, local-only/privacy network surface, Tauri IPC/capability surface, Rust risk scan, bundle asset scan, unsigned-beta packaging state, installed-artifact smoke, orphan-process check, and docs-reality check.

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

For this beta, do **not** set the `APPLE_*` environment variables described in `BUILD.md`. With those env vars unset, `tauri build` produces an unsigned/ad-hoc local artifact (no notarization step, no upload). The build typically completes in 1–3 minutes.

When it finishes, confirm both files exist:

```
dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg
dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg.sha256
```

Verify the DMG is not Developer ID signed/notarized with:

```bash
codesign -dv dist-artifacts/TurboTalk-<new-version>-macos-arm64.dmg
```

The `Authority` line may be absent, or the command may report that the code object is not signed at all. That is acceptable for this unsigned beta. A `Notarized Developer ID` source means somebody set the `APPLE_*` env vars — back out and rebuild without them.

### Build procedure — Windows

Build host: Windows 10 (1809+) or Windows 11, x64.

> **Runtime gap (current):** `npm run fetch-sidecars` now downloads the bundled Whisper sidecar for Windows, so packaging can complete. The installer is still not a usable dictation beta until Windows hotkey + paste implementations replace the unsupported stubs.

Required toolchain (one-time setup on the build host):

- Rust toolchain (rustup) with the `x86_64-pc-windows-msvc` target.
- Visual Studio Build Tools 2022 with the "Desktop development with C++" workload (provides `cl.exe`, the MSVC linker, and the Windows SDK).
- Node.js 20+.
- WebView2 SDK (usually picked up automatically; included with the VS Build Tools workload).

From a Windows host:

```powershell
npm install
npm run package
```

Expected artifacts:

```
dist-artifacts/TurboTalk-<new-version>-windows-x64-setup.exe
dist-artifacts/TurboTalk-<new-version>-windows-x64-setup.exe.sha256
```

The `.exe` is an NSIS installer. **It is unsigned.** End users will see SmartScreen "Windows protected your PC" on first run — that is documented in `README.md`. Do **not** sign the installer for this beta. Do not publish it as a working dictation artifact until Windows hotkey + paste are implemented and smoke-tested.

### Build procedure — Linux (X11)

Build host: Ubuntu 22.04 or any Debian-derivative with the Tauri 2 prereqs installed. Equivalent packages exist on Fedora/Arch — see <https://tauri.app/start/prerequisites/>.

> **Prerequisite gap (current):** the bundled Whisper sidecar binary for Linux (`src-tauri/binaries/whisper-cli-x86_64-unknown-linux-gnu`) is not yet in the repo, and Linux hotkey/paste are not validated. Until those land, Linux remains excluded from the release matrix. The procedure below is correct in shape and will work once the binary/runtime path lands.

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

On tag pushes, `.github/workflows/release.yml` builds the matrix artifacts and creates a draft GitHub release automatically. Review the draft release, verify artifacts, and run the installed-artifact smoke test before publishing it.

For manual local publishing, write the release notes for this version into `RELEASE_NOTES.md` at the repo root using the template at the bottom of this file, then publish.

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

For a macOS-only usable release (the current runtime state), attach only the two macOS files. Drop the rest of the lines.

`RELEASE_NOTES.md` is a scratch file per release — do not commit it.

## Step 5 — Update SESSION-STATUS.md

Add a single line under "Where We Are":

> Released v<new-version> beta on <YYYY-MM-DD>.

Commit as `chore(status): record v<new-version> release`.

## Update policy

TurboTalk ships a **manual check-for-updates** button in the Settings tab. The Tauri updater plugin is wired and will check `https://github.com/eldo9000/TurboTalk-App/releases/latest/download/latest.json` when the user clicks "Check for updates." It does **not** check automatically on launch and does not run in the background — the check is strictly user-initiated and throttled to once per week via localStorage.

This is not a full auto-updater. Users who never click the button will not receive update prompts. Communicating new releases via direct message / release notes is still the primary distribution path for this beta.

Before enabling background auto-update we need: (1) a long-lived updater signing key with a documented secure-custody plan, (2) a stable artifact-hosting URL that we control, (3) a written key-rotation/loss procedure. Until those three exist, do not change the updater from its current manual-check-only mode.

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

**Install — macOS:** Download `TurboTalk-<version>-macos-arm64.dmg`, drag `Turbo Talk.app` into `/Applications`, **right-click → Open** on first launch (the GitHub-downloaded beta is unsigned/not notarized and quarantined by macOS), and grant Microphone and Accessibility permissions when prompted. If macOS still blocks it, use System Settings → Privacy & Security → Open Anyway.

**Install — Windows:** Download `TurboTalk-<version>-windows-x64-setup.exe`. SmartScreen will warn that the installer is unsigned — click **More info → Run anyway**. Run the installer and launch from the Start menu. WebView2 runtime is required (preinstalled on Windows 11; Windows 10 users may need <https://developer.microsoft.com/microsoft-edge/webview2/>).

**Install — Linux:** Download `TurboTalk-<version>-linux-x64.AppImage`, `chmod +x`, and run. Requires FUSE (`libfuse2` on Debian/Ubuntu) and an X11 session — Wayland is not supported.

**What's in this release**
- <one bullet per user-visible change>

**Known limitations**
- macOS: Apple Silicon only; unsigned/not notarized beta (Gatekeeper warning on first launch from downloaded artifacts).
- Windows: packaging only; hotkey + paste still unsupported; unsigned installer (SmartScreen warning on first run).
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
