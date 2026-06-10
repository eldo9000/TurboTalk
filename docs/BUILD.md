# TurboTalk Build

How to produce TurboTalk beta artifacts. Today the only usable runtime beta is
**macOS arm64**. CI can also package a Windows installer with the bundled
Whisper sidecar, but Windows hotkey + paste are still unsupported stubs, so it
is not a working dictation release yet. Linux builds remain deferred.

> For the full release procedure (versioning, tagging, publishing, release
> notes), see `RELEASING.md`.

## Build a macOS arm64 beta DMG

Prerequisites (install once):

- macOS on Apple Silicon (arm64).
- Xcode Command Line Tools (`xcode-select --install`).
- Rust toolchain via `rustup` (stable, host triple `aarch64-apple-darwin`).
- Node.js 22+ and npm.
- Repo dependencies installed: `npm install` from the repo root.
- Whisper sidecar + dylibs present at `src-tauri/binaries/` (the preflight
  script will fail loudly if they are missing).

From a clean checkout:

```bash
npm install
npm run package
```

`npm run package` chains:

1. `npm run preflight` — verifies the bundled Whisper sidecar/dylibs exist and
   checks every committed macOS native binary against the SHA-256 manifest at
   `src-tauri/binaries/MANIFEST.sha256`.
2. `tauri build` — produces the unsigned/ad-hoc `.app` and `.dmg` under
   `src-tauri/target/release/bundle/`.
3. `node scripts/rename-artifact.mjs` — copies the DMG to a stable,
   convention-named path and writes a matching `.sha256` checksum file.

Expected output paths:

```
dist-artifacts/TurboTalk-<version>-macos-arm64.dmg
dist-artifacts/TurboTalk-<version>-macos-arm64.dmg.sha256
```

Verify the DMG is intact with
`shasum -a 256 -c dist-artifacts/TurboTalk-<version>-macos-arm64.dmg.sha256`
(expect `TurboTalk-<version>-macos-arm64.dmg: OK`).

`dist-artifacts/` is gitignored. The original Tauri-named DMG remains in
`target/release/bundle/dmg/` if you need it.

## Artifact naming convention

```
TurboTalk-<version>-<os>-<arch>.<ext>
```

| Field | Source | Example |
|---|---|---|
| `<version>` | `package.json` `version` (must match `src-tauri/tauri.conf.json`) | `0.8.6` |
| `<os>` | platform identifier | `macos`, `windows`, `linux` |
| `<arch>` | user-facing arch label | `arm64`, `x64` |
| `<ext>` | platform-native installer | `dmg`, `exe`, `AppImage` |

The version is read from `package.json` at rename time, so bumping the version
in both `package.json` and `src-tauri/tauri.conf.json` is the only change needed
to retag artifacts.

## Smoke test the artifact

After `npm run package` finishes, verify the DMG actually works. This is
the installed-artifact proof gate from `SMOKE-TEST.md` — until you have done
this, the build is not proven.

- Open `dist-artifacts/TurboTalk-<version>-macos-arm64.dmg`.
- Drag `Turbo Talk.app` to `/Applications`.
- GitHub-downloaded beta builds are unsigned/not notarized and quarantined by
  macOS, so launch with right-click → Open the first time and accept the
  Gatekeeper warning. If macOS still blocks it, use System Settings → Privacy &
  Security → Open Anyway.
- Grant **Microphone** and **Accessibility / Input Monitoring** when the
  OS prompts.
- Hold the configured push-to-talk hotkey, dictate one short phrase
  (e.g. "hello world") into a focused text field (TextEdit is fine),
  release.
- Verify the phrase pastes into the focused app.
- Quit the app cleanly via the tray menu.

If any step fails, the build is not beta-ready — file the failure in
`SESSION-STATUS.md` before retrying.

## Build a Windows x64 beta on a Windows host

Prerequisites:

- Windows 10/11 x64 with WebView2 runtime present.
- Rust toolchain via `rustup` (stable, host triple
  `x86_64-pc-windows-msvc`).
- Visual Studio Build Tools 2022 with the "Desktop development with C++"
  workload (MSVC linker + Windows SDK).
- Node.js 22+ and npm.

From a clean checkout in PowerShell:

```powershell
npm install
npm run fetch-sidecars
npm run package
```

`npm run fetch-sidecars` downloads the pinned upstream whisper.cpp
release zip (version + sha256 are baked into
`scripts/fetch-sidecars.mjs`), verifies the hash, and extracts
`whisper-cli.exe` plus the runtime DLLs into `src-tauri/binaries/`.
Companion DLLs are bundled into the installer via
`src-tauri/tauri.windows.conf.json` (`bundle.resources`).

`npm run fetch-onnxruntime` downloads the
Microsoft.ML.OnnxRuntime NuGet package, verifies its SHA-256 against
a pinned digest in the script, and extracts `onnxruntime.dll`.

The fetch step downloads sidecars only on Windows hosts; it is a no-op on
non-Windows hosts. Packaging can complete, but the resulting app is still not a
working Windows dictation beta until hotkey + paste implementations land.

Linux beta artifacts are still blocked — Linux is excluded from
`.github/workflows/release.yml` until the rdev hotkey + paste paths are
validated on real X11 hardware.

## Cross-platform — what's still missing

- **Real `hotkey` and `paste` implementations** for non-mac platforms.
  Today `src-tauri/src/hotkey.rs` and `src-tauri/src/paste.rs` ship
  unsupported stubs off-mac. Win/Linux impls are TASK-25 / TASK-26.
- **Linux Whisper sidecar**: upstream whisper.cpp does not publish a
  pre-built Linux binary. Either build from source in CI or ship a
  static binary in a separate prebuilds repo before re-enabling Linux
  in the release matrix.
- **Linux artifact rename path**: `scripts/rename-artifact.mjs` handles macOS
  and Windows today. Linux (`-linux-x64.AppImage`) can be added once the source
  paths produced by `tauri build` on that target are known.

## Release build (signed + notarized)

The default `npm run package` build is unsigned/ad-hoc
(`signingIdentity: "-"` in `src-tauri/tauri.conf.json`) and is fine for
the developer's own machine. Downloaded artifacts from GitHub carry Apple's
quarantine flag; any Mac that has not run TurboTalk before may refuse to launch
it:

> "Turbo Talk" cannot be opened because the developer cannot be verified.

For external distribution (beta testers, friends, anyone who is not you),
the DMG must be signed with a real **Developer ID Application**
certificate **and** notarized by Apple. This section walks through that
end-to-end. Read it once front to back before starting — every command
and env var you need is here.

### One-time setup

1. **Apple Developer account.** $99/year at
   <https://developer.apple.com/programs/>. Required to obtain a
   Developer ID Application certificate.

2. **Developer ID Application certificate.**
   - In **Xcode** → Settings → Accounts → select your team → Manage
     Certificates → **+** → **Developer ID Application**.
   - This installs the cert and its private key into your **login**
     keychain. Verify with:
     ```bash
     security find-identity -v -p codesigning
     ```
     You should see one line like
     `1) ABCDEF1234... "Developer ID Application: Your Name (TEAMID1234)"`.
     The 40-char hex hash on the left is what `APPLE_SIGNING_IDENTITY`
     wants. The full quoted string also works.

3. **Apple Team ID.** 10-character string visible in
   <https://developer.apple.com/account> → Membership Details → Team ID,
   and embedded in the parenthesised suffix of the cert's common name
   (`TEAMID1234` above).

4. **App-specific password** (for `APPLE_PASSWORD`).
   - Go to <https://appleid.apple.com> → Sign-In and Security →
     App-Specific Passwords → **+**.
   - Name it something like `turbotalk-notary`. Save the generated
     `xxxx-xxxx-xxxx-xxxx` string somewhere safe — Apple will not show
     it again.
   - Your normal Apple ID password will **not** work for notarization.

   Alternative (recommended for CI, optional for local builds): an
   **App Store Connect API key**. Generate at
   <https://appstoreconnect.apple.com/access/integrations/api>, download
   the `.p8` file, and use the `APPLE_API_KEY` / `APPLE_API_ISSUER` /
   `APPLE_API_KEY_PATH` env vars in step 3 below instead of
   `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`.

### Required env vars

Set these in your shell (do **not** commit them anywhere — `.env*` is
gitignored, but the safer pattern is to keep them in your shell rc or a
secret manager and never write them to a file in the repo):

| Var | Value | Where from |
|---|---|---|
| `APPLE_SIGNING_IDENTITY` | The 40-char SHA-1 hash **or** the full `"Developer ID Application: Your Name (TEAMID1234)"` string | `security find-identity -v -p codesigning` (step 2) |
| `APPLE_ID` | Your Apple ID email | The address you sign in to App Store Connect with |
| `APPLE_PASSWORD` | App-specific password | Step 4 above (`xxxx-xxxx-xxxx-xxxx`) |
| `APPLE_TEAM_ID` | 10-char team identifier | Step 3 above |

If `APPLE_SIGNING_IDENTITY` is set, Tauri overrides the `"-"` in
`tauri.conf.json` for that build — so dev builds (`npm run tauri dev`)
remain unaffected as long as you do not export the var globally.

### Build invocation

From the repo root, with all four env vars exported in the current
shell:

```bash
npm run package
```

That runs the same chain as the dev build (preflight → `tauri build` →
rename), but because `APPLE_SIGNING_IDENTITY` is set, Tauri:

1. Signs the `.app` (and every embedded sidecar/dylib under `Resources/`
   and `Frameworks/`) with the Developer ID cert. The bundled
   `whisper-cli` and the three `lib*.dylib` files in
   `src-tauri/binaries/` are signed automatically as part of the
   recursive `codesign --deep` pass on the `.app`.
2. Builds the DMG.
3. Submits the DMG to Apple's notary service (via `notarytool`), waits
   for the verdict, and **staples** the resulting ticket to the DMG so
   Gatekeeper can verify offline.

The notarization upload + scan typically takes **5–15 minutes**. The
`tauri build` invocation will appear to hang during that window — it is
waiting on Apple, not stuck.

Expected output (same path as the dev build):

```
dist-artifacts/TurboTalk-<version>-macos-arm64.dmg
```

### Verify the result

Run these on the build machine **before** sending the DMG to anyone:

```bash
# Verifies the DMG itself is signed and notarized.
spctl -a -t open --context context:primary-signature -v \
  dist-artifacts/TurboTalk-<version>-macos-arm64.dmg

# Verifies the .app inside the DMG (mount the DMG first).
hdiutil attach dist-artifacts/TurboTalk-<version>-macos-arm64.dmg
spctl -a -vv "/Volumes/Turbo Talk/Turbo Talk.app"
codesign -dv --verbose=4 "/Volumes/Turbo Talk/Turbo Talk.app"
hdiutil detach "/Volumes/Turbo Talk"
```

Healthy `spctl` output for the DMG looks like:

```
dist-artifacts/TurboTalk-<version>-macos-arm64.dmg: accepted
source=Notarized Developer ID
```

If you see `source=Developer ID` (no "Notarized"), the staple failed —
re-run `xcrun stapler staple dist-artifacts/TurboTalk-<version>-macos-arm64.dmg`
and re-verify.

If you see `rejected`, the cert chain is bad or the notary submission
was refused. Check the most recent submission with:

```bash
xcrun notarytool history --apple-id "$APPLE_ID" \
  --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID"
xcrun notarytool log <submission-id> --apple-id "$APPLE_ID" \
  --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID"
```

### Real-machine proof (the actual success signal)

The above `spctl` command on the build machine is necessary but not
sufficient — the build machine has the cert in its keychain and may
trust the binary for the wrong reason. The real proof:

- Copy the DMG to a Mac that has **never** run TurboTalk.
- Open the DMG, drag `Turbo Talk.app` to `/Applications`.
- Double-click (do **not** right-click → Open). It should launch
  without the "developer cannot be verified" warning. The first
  launch will still prompt for Microphone and Accessibility / Input
  Monitoring permissions — that is expected and unrelated to signing.

If the second-machine launch shows the Gatekeeper warning, the build
is not beta-ready. File the failure in `SESSION-STATUS.md`.

### Entitlements

The hardened runtime entitlements live at `src-tauri/entitlements.plist`
and are referenced from `tauri.conf.json` `bundle.macOS.entitlements`.
The current file grants:

- `com.apple.security.device.audio-input` — required for `cpal`
  microphone capture under hardened runtime.
- `com.apple.security.automation.apple-events` — required for the
  `osascript`-based paste path in `src-tauri/src/paste.rs`.
- `com.apple.security.cs.allow-jit` — required by the WKWebView used by
  Tauri.
- `com.apple.security.cs.allow-unsigned-executable-memory` and
  `com.apple.security.cs.disable-library-validation` — required for the
  bundled whisper.cpp sidecar + dylibs to load and execute under
  hardened runtime.

Do not add entitlements that are not strictly needed — every additional
entitlement is a notarization risk and a security surface.
