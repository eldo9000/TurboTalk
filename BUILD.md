# TurboTalk Build

How to produce the first-beta TurboTalk artifact. Today that means **macOS
arm64 only**. Cross-platform builds are deferred — see the bottom of this
file for what would need to land first.

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

1. `npm run preflight` — verifies the bundled Whisper sidecar/dylibs exist.
2. `tauri build` — produces the signed (ad-hoc) `.app` and `.dmg` under
   `src-tauri/target/release/bundle/`.
3. `node scripts/rename-artifact.mjs` — copies the DMG to a stable,
   convention-named path.

Expected output path:

```
dist-artifacts/TurboTalk-0.0.1-macos-arm64.dmg
```

`dist-artifacts/` is gitignored. The original Tauri-named DMG remains in
`src-tauri/target/release/bundle/dmg/` if you need it.

## Artifact naming convention

```
TurboTalk-<version>-<os>-<arch>.<ext>
```

| Field | Source | Example |
|---|---|---|
| `<version>` | `package.json` `version` (must match `src-tauri/tauri.conf.json`) | `0.0.1` |
| `<os>` | platform identifier | `macos`, `windows`, `linux` |
| `<arch>` | user-facing arch label | `arm64`, `x64` |
| `<ext>` | platform-native installer | `dmg`, `exe`, `AppImage` |

Today only the macOS arm64 variant is produced. The version is read from
`package.json` at rename time, so bumping the version in both
`package.json` and `src-tauri/tauri.conf.json` is the only change needed
to retag artifacts.

## Smoke test the artifact

After `npm run package` finishes, verify the DMG actually works. This is
the Block 2 proof gate from `BETA-AUDIT-ROADMAP.md` — until you have
done this, the build is not proven.

- Open `dist-artifacts/TurboTalk-0.0.1-macos-arm64.dmg`.
- Drag `Turbo Talk.app` to `/Applications`.
- The build is ad-hoc signed, so launch with right-click → Open the
  first time and accept the Gatekeeper warning.
- Grant **Microphone** and **Accessibility / Input Monitoring** when the
  OS prompts.
- Hold the configured push-to-talk hotkey, dictate one short phrase
  (e.g. "hello world") into a focused text field (TextEdit is fine),
  release.
- Verify the phrase pastes into the focused app.
- Quit the app cleanly via the tray menu.

If any step fails, the build is not beta-ready — file the failure in
`SESSION-STATUS.md` before retrying.

## Cross-platform packaging — deferred

Windows and Linux beta artifacts are blocked. Before adding npm scripts
or build commands for those platforms, the following must land:

- **Whisper sidecars** with target-triple suffixes in `src-tauri/binaries/`:
  `whisper-cli-x86_64-pc-windows-msvc.exe` (and DLLs) and
  `whisper-cli-x86_64-unknown-linux-gnu` (and `.so` deps or a static
  build). Today only the `aarch64-apple-darwin` sidecar is bundled.
- **Real `hotkey` and `paste` implementations** for each platform. The
  current `src-tauri/src/hotkey.rs` and `src-tauri/src/paste.rs` are
  macOS-only; non-mac targets must either ship a real implementation or
  surface a clear "unsupported platform" error rather than silently
  pretending to work.
- **Platform-specific build prerequisites**: WebView2 runtime + MSVC
  toolchain on Windows; WebKitGTK + libsoup + appindicator dev packages
  on Linux (varies by distro). Tauri's prerequisite docs cover the full
  list per OS.
- **Preflight script extension**: `scripts/preflight.mjs` is currently
  macOS-only. It needs per-platform required-asset lists before
  `npm run package` is meaningful elsewhere.
- **Rename helper extension**: `scripts/rename-artifact.mjs` only emits
  `macos-arm64.dmg` today. Adding Windows (`.exe`) and Linux
  (`.AppImage`) outputs is straightforward once the source paths
  produced by `tauri build` on those targets are known.

Until all of the above are in place, `npm run package` is a macOS-only
command and any cross-platform claim in the README would be dishonest.

## Signing & notarization — deferred

The current build is ad-hoc signed (`signingIdentity: "-"` in
`src-tauri/tauri.conf.json`) and not notarized. This is intentional for
beta-1: distribution is "you have to right-click and Open the first
time." Real Developer ID signing + Apple notarization is Block 5 of
`BETA-AUDIT-ROADMAP.md` and is not part of this build flow yet.
