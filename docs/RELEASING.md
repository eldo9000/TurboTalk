# Releasing TurboTalk

> **1.0 release: unsigned/not notarized.** GitHub-downloaded macOS artifacts carry Apple's download quarantine flag, so users will see Gatekeeper warnings and may need right-click → Open or System Settings → Privacy & Security → Open Anyway. The Windows `.exe` has no Authenticode signature and will trigger SmartScreen.
>
> **Skipping to signed?** The CI workflow conditionally uses Developer ID signing + notarization when the `APPLE_SIGNING_IDENTITY` secret is configured — see the [Signing secrets reference](#signing-secrets-reference) below. Follow the release procedure here; CI will detect the credentials and produce signed artifacts automatically.

## Scope

This is the procedure for cutting a TurboTalk 1.0 release.

- **macOS arm64** (Apple Silicon) — supported for 1.0.
- **Windows x64** — supported for 1.0.
- **Linux x64 (X11)** — excluded from 1.0; this is the 2.0 Linux track until Linux sidecar, hotkey, and paste are validated on real hardware.

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
- Runtime Whisper `.bin` and Parakeet ONNX model downloads
  are verified against pinned SHA-256 hashes before being persisted to disk.
- The release scan pack in `RELEASE-READINESS.md` has no undocumented blockers:
  version consistency, updater/manual-update consistency, local-only/privacy network surface, Tauri IPC/capability surface, Rust risk scan, bundle asset scan, unsigned packaging state, installed-artifact smoke, orphan-process check, and docs-reality check.

- `cargo build` (or `npm run package`) confirms that `TURBOTALK_BUGREPORT_TG_TOKEN`
  and `TURBOTALK_BUGREPORT_TG_CHAT` are **not** embedded in the binary.
  These environment variables are gated behind the `dev-telegram-bugreport`
  Cargo feature (off by default). To verify the binary has no embedded secrets,
  run `strings <binary> | rg 'TURBOTALK_BUGREPORT'` — it must return nothing.
  The release CI does not set these variables and does not enable the feature
  (see `release.yml` and `Cargo.toml`).

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

Run the per-platform build section that matches your build host. Each section below produces an unsigned artifact and its `.sha256` companion file in `build/`.

### Build procedure — macOS

Build host: macOS on Apple Silicon.

```bash
npm install
npm run package
```

For 1.0, do **not** set the `APPLE_*` environment variables described in `BUILD.md`. With those env vars unset, `tauri build` produces an unsigned/ad-hoc local artifact (no notarization step, no upload). The build typically completes in 1–3 minutes.

When it finishes, confirm both files exist:

```
build/TurboTalk-<new-version>-macos-arm64.dmg
build/TurboTalk-<new-version>-macos-arm64.dmg.sha256
```

Verify the DMG is not Developer ID signed/notarized with:

```bash
codesign -dv build/TurboTalk-<new-version>-macos-arm64.dmg
```

The `Authority` line may be absent, or the command may report that the code object is not signed at all. That is acceptable for this unsigned 1.0 release. A `Notarized Developer ID` source means somebody set the `APPLE_*` env vars; back out and rebuild without them.

### Build procedure — Windows

Build host: Windows 10 (1809+) or Windows 11, x64.

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
build/TurboTalk-<new-version>-windows-x64-setup.exe
build/TurboTalk-<new-version>-windows-x64-setup.exe.sha256
```

The `.exe` is an NSIS installer. **It is unsigned.** End users will see SmartScreen "Windows protected your PC" on first run; that is documented in `README.md`. Do **not** sign the installer for 1.0. Do not publish it until the Windows installed-artifact smoke test passes.

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
build/TurboTalk-<new-version>-linux-x64.AppImage
build/TurboTalk-<new-version>-linux-x64.AppImage.sha256
```

The AppImage is **unsigned**. Users `chmod +x` and run it directly. There is no `.deb` or `.rpm` for this future Linux release; one AppImage covers all distros that have FUSE installed.

## Step 3 — Verify artifacts

For each artifact you intend to publish, verify the matching `.sha256` and run the **per-platform installed-artifact smoke test**:

- macOS: `SMOKE-TEST.md` → "macOS smoke test" + the installed-artifact subsection.
- Windows: `SMOKE-TEST.md` → "Windows smoke test" + the installed-artifact subsection.
- Linux: only for 2.0 work, `SMOKE-TEST.md` → "Linux smoke test (X11)".

Every numbered step in the smoke test for each platform you are publishing must pass before tagging.

## Step 4 — Tag and publish

```bash
git tag v<new-version>
git push origin main --tags
```

On tag pushes, `.github/workflows/release.yml` builds the matrix artifacts and creates a draft GitHub release automatically. Review the draft release, verify artifacts, and run the installed-artifact smoke test before publishing it.

For manual local publishing, write the release notes for this version into `RELEASE_NOTES.md` at the repo root using the template at the bottom of this file, then publish.

For a 1.0 release, attach the macOS and Windows artifacts plus their checksums:

```bash
gh release create v<new-version> \
  --title "TurboTalk <new-version>" \
  --notes-file RELEASE_NOTES.md \
  build/TurboTalk-<new-version>-macos-arm64.dmg \
  build/TurboTalk-<new-version>-macos-arm64.dmg.sha256 \
  build/TurboTalk-<new-version>-windows-x64-setup.exe \
  build/TurboTalk-<new-version>-windows-x64-setup.exe.sha256
```

Linux artifacts are not part of the 1.0 release. Add them only after the 2.0 Linux smoke path is proven.

`RELEASE_NOTES.md` is a scratch file per release — do not commit it.

## Step 5 — Update SESSION-STATUS.md

Add a single line under "Where We Are":

> Released v<new-version> on <YYYY-MM-DD>.

Commit as `chore(status): record v<new-version> release`.

## Signing secrets reference

The CI pipeline (`release.yml`) checks for the following secrets at workflow start and adjusts signing behavior accordingly. None of these are required for unsigned 1.0 builds; all are optional until you configure them.

| Secret | Platform | Purpose | Required for signed release? |
|--------|----------|---------|------------------------------|
| `APPLE_SIGNING_IDENTITY` | macOS | Developer ID Application cert hash or full name | Yes (macOS) |
| `APPLE_ID` | macOS | Apple ID email for notarization upload | Yes (macOS notarization) |
| `APPLE_PASSWORD` | macOS | App-specific password for notarytool | Yes (macOS notarization) |
| `APPLE_TEAM_ID` | macOS | 10-character Apple Team ID | Yes (macOS notarization) |
| `WINDOWS_SIGNING_CERTIFICATE` | Windows | Base64-encoded PFX/P12 Authenticode cert | Yes (Windows) |
| `WINDOWS_SIGNING_PASSWORD` | Windows | PFX/P12 private-key password | Yes (Windows) |

### macOS: how CI detects signing

The `APPLE_SIGNING_IDENTITY` secret is the gate. When present:
1. Tauri exports `APPLE_SIGNING_IDENTITY` during `tauri build`, producing a Developer-ID-signed `.app` and `.dmg`.
2. Tauri also calls `notarytool` (using `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`) to submit the DMG to Apple's notary service and staple the ticket.
3. The `Ad-hoc codesign` step is skipped — re-signing with ad-hoc would strip the Developer ID signature.
4. An additional `Verify Developer ID signing and notarization` step runs `codesign -dv` and `spctl --assess` on the `.app` and DMG.

When `APPLE_SIGNING_IDENTITY` is absent, the pipeline falls back to ad-hoc signing (the 1.0 behavior). All steps remain green — just without notarization.

### Windows: how CI detects signing

The `WINDOWS_SIGNING_CERTIFICATE` secret is the gate. When present with a base64-decoded PFX:
1. After `tauri build` finishes, `signtool sign /fd SHA256 /a /f <cert>` is called on the NSIS installer.
2. The certificate tempfile is cleaned up immediately after signing.

When absent, the signing step prints a skip message and exits cleanly. The installer is uploaded unsigned.

### Setting up secrets

Secrets are configured at **GitHub repo → Settings → Secrets and variables → Actions**. Do not commit secret values anywhere in the repo — `.env*` is gitignored, but the safer pattern is to never write secrets to a file in the checkout.

For local (non-CI) signing, see `BUILD.md` → [Release build (signed + notarized)](./BUILD.md#release-build-signed--notarized).

### Verifying a signed artifact

After a signed CI run completes, download the DMG and run:

```bash
codesign -dv --verbose=4 "Turbo Talk.app"
spctl --assess --type execute --verbose=4 "Turbo Talk.app"
spctl -a -t open --context context:primary-signature -v TurboTalk-<version>-macos-arm64.dmg
```

Expected `spctl` output for the DMG:
```
TurboTalk-<version>-macos-arm64.dmg: accepted
source=Notarized Developer ID
```

For Windows:
```powershell
Get-AuthenticodeSignature TurboTalk-<version>-windows-x64-setup.exe
```

Expected: `SignerCertificate` chain resolves to the code-signing CA and `Status` is `Valid`.

## Update policy

TurboTalk ships a **manual check-for-updates** button in the Settings tab. The Tauri updater plugin is wired and will check `https://github.com/eldo9000/TurboTalk/releases/latest/download/latest.json` when the user clicks "Check for updates." It does **not** check automatically on launch and does not run in the background — the check is strictly user-initiated and throttled to once per week via localStorage.

This is not a full auto-updater. Users who never click the button will not receive update prompts. Communicating new releases via direct message / release notes is still the primary distribution path for 1.0.

Before enabling background auto-update we need: (1) a long-lived updater signing key with a documented secure-custody plan, (2) a stable artifact-hosting URL that we control, (3) a written key-rotation/loss procedure. Until those three exist, do not change the updater from its current manual-check-only mode.

## Signed release paths

### macOS (CI, preferred)

Configure the required [signing secrets](#signing-secrets-reference) (`APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`) in the GitHub repo. Then follow the normal [tag and publish](#step-4--tag-and-publish) flow. The `release.yml` build job detects the credentials automatically and produces a Developer-ID-signed, notarized DMG with a matching updater tarball.

No code changes needed — `tauri.conf.json` keeps `signingIdentity: "-"` because CI overrides it via the `APPLE_SIGNING_IDENTITY` environment variable.

### macOS (local)

For local signed builds (not CI):

1. Install the Developer ID Application certificate in your login keychain (Xcode → Settings → Accounts → Manage Certificates → + → Developer ID Application).
2. Set the four `APPLE_*` environment variables described in `BUILD.md` → "Release build (signed + notarized)" → "Required env vars".
3. Run `npm run package` from the Mac with the cert installed.
4. The notarization upload + scan typically takes 5–15 minutes; the build will appear to hang during that window.
5. Verify with:
   ```
   spctl -a -t open --context context:primary-signature -v build/TurboTalk-<version>-macos-arm64.dmg
   ```
   Expect `accepted` and `source=Notarized Developer ID`.

### Windows

For signed Windows installers, configure `WINDOWS_SIGNING_CERTIFICATE` and `WINDOWS_SIGNING_PASSWORD` as GitHub secrets. The CI pipeline applies Authenticode signing via `signtool` after the build completes.

For local Windows signing, use:
```powershell
signtool sign /fd SHA256 /a /f <path-to.pfx> /p <password> build/TurboTalk-<version>-windows-x64-setup.exe
```

### Linux

Linux AppImage signing is not set up. Future work: GPG-signed `.zsync` + detached `.sig`. Linux artifacts are deferred to the 2.0 Linux track.

### `tauri.conf.json` — why `signingIdentity` stays `"-"`

The committed config keeps `signingIdentity: "-"` (ad-hoc). This is deliberate:
- Local `npm run package` stays DMG-only, fast, and does not require Apple credentials.
- CI overrides via the `APPLE_SIGNING_IDENTITY` environment variable when configured.
- Dev builds (`npm run tauri dev`) are unaffected regardless of environment variables.

## Release notes template

Copy this into `RELEASE_NOTES.md` and fill in the `<placeholders>`. For a single-platform release, drop the rows for platforms you are not publishing.

```markdown
## TurboTalk <version>

**Platforms in this release:**
- macOS 12+ on Apple Silicon (arm64).
- Windows 10 (1809+) / 11 on x64.
- Linux is not included in the 1.0 release.

**Install — macOS:** Download `TurboTalk-<version>-macos-arm64.dmg`, drag `Turbo Talk.app` into `/Applications`, **right-click → Open** on first launch (the GitHub-downloaded release is unsigned/not notarized and quarantined by macOS), and grant Microphone and Accessibility permissions when prompted. If macOS still blocks it, use System Settings → Privacy & Security → Open Anyway.

**Install — Windows:** Download `TurboTalk-<version>-windows-x64-setup.exe`. SmartScreen will warn that the installer is unsigned — click **More info → Run anyway**. Run the installer and launch from the Start menu. WebView2 runtime is required (preinstalled on Windows 11; Windows 10 users may need <https://developer.microsoft.com/microsoft-edge/webview2/>).

**Install — Linux:** Linux support is deferred to the 2.0 track.

**What's in this release**
- <one bullet per user-visible change>

**Known limitations**
- macOS: Apple Silicon only; unsigned/not notarized (Gatekeeper warning on first launch from downloaded artifacts).
- Windows: x64 only; unsigned installer (SmartScreen warning on first run).
- Linux: not included in 1.0.
- <other known issues>

**Privacy and data** — see `PRIVACY.md`. To delete all local TurboTalk data, follow `PRIVACY.md` → "How to delete everything".

**Verify the download**

    # macOS:
    shasum -a 256 -c TurboTalk-<version>-macos-arm64.dmg.sha256

    # Windows (PowerShell):
    Get-FileHash -Algorithm SHA256 TurboTalk-<version>-windows-x64-setup.exe

Compare the Windows hash output against the contents of `TurboTalk-<version>-windows-x64-setup.exe.sha256`.

**Feedback** — file an issue at https://github.com/eldo9000/TurboTalk/issues
```

## Hotfix path

Hotfixes follow the same procedure: bump the patch version, rebuild on the relevant host(s), re-verify, re-publish. There is no separate fast-path.

## Rollback

We do not delete published GitHub releases. If a release is broken, mark it as **pre-release** (`gh release edit v<version> --prerelease`) so the "Latest" badge moves off it, then publish a corrected release with a bumped patch version. The broken release stays visible for forensics.
