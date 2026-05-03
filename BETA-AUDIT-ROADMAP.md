# TurboTalk Beta Audit Roadmap

Purpose: prepare TurboTalk for a first beta release, especially if the target is
Windows / macOS / Linux rather than macOS-only. This is an audit roadmap, not a
task implementation spec. Agents should work one block at a time, preserve the
current macOS happy path, and update `SESSION-STATUS.md` with what was proven.

## Current Beta Reality

TurboTalk is currently a working macOS dictation app with a macOS-shaped core:

- Hotkey is implemented with `CGEventTap` in `src-tauri/src/hotkey.rs`.
- Paste is implemented with `osascript` + clipboard in `src-tauri/src/paste.rs`.
- Bundled Whisper sidecar is Apple Silicon only:
  `src-tauri/binaries/whisper-cli-aarch64-apple-darwin`.
- Bundled Whisper libraries are macOS `.dylib` files.
- Tauri config sets `macOSPrivateApi: true`.

So a cross-platform beta is not just a packaging exercise. The first release
decision should be explicit:

- **Mac beta first:** fastest and lowest-risk. Finish signing/notarization,
  privacy copy, and stability burn-in.
- **True cross-platform beta:** requires platform abstraction for hotkey/paste,
  platform-specific Whisper sidecars/libraries, packaging verification on each
  OS, and a wider failure-mode test matrix.

Recommended sequencing: Blocks 1 → 2 → 3 → 4 → 5. If shipping mac-only first,
do Blocks 2, 3, 4, and the macOS part of Block 5 before release; keep Block 1
as the promotion path.

## Block 1 — Platform Compatibility Audit

Goal: make the codebase honest about what is mac-only, what can compile on all
desktop targets, and what needs platform-specific implementations.

### Questions To Answer

- Does `cargo check` compile for Windows and Linux targets?
- Which modules need `#[cfg(target_os = "...")]` boundaries?
- What is the minimum viable Windows/Linux implementation for hotkey and paste?
- Is the product claiming cross-platform behavior before the implementation
  exists?

### Current Suspect Areas

- `src-tauri/src/hotkey.rs`
  - Imports CoreFoundation/CoreGraphics directly.
  - Uses `CGEventTap`, `CFRunLoop`, `CGEventFlags`, and macOS keycodes.
  - Suggested direction: split into `hotkey/macos.rs`, `hotkey/windows.rs`,
    `hotkey/linux.rs`, and a small cross-platform facade.
  - For beta, Windows/Linux can start as a clear unsupported error if the app is
    mac-only, but should not fail compilation accidentally.

- `src-tauri/src/paste.rs`
  - `frontmost_app()` is already cfg-gated, but `paste()` itself shells out to
    `osascript` unconditionally.
  - Suggested direction: define a paste trait/facade:
    `paste::paste(text)`, `paste::frontmost_app()`, and maybe
    `paste::capability_status()`.
  - macOS implementation can stay clipboard + Cmd+V.
  - Windows likely needs clipboard + Ctrl+V via `enigo` or native input APIs.
  - Linux likely needs separate handling for X11 vs Wayland. Wayland global
    input/paste automation is more restricted, so document supported desktop
    environments honestly.

- `src-tauri/tauri.conf.json`
  - `macOSPrivateApi: true` is mac-specific and required for current hotkey.
  - `externalBin` currently references one sidecar base name, but actual
    platform files must exist per target triple.

- `src-tauri/binaries/`
  - Currently Apple Silicon binary and macOS dylibs only.
  - Tauri sidecar docs expect target-triple-suffixed binaries for supported
    architectures.

### Suggested Deliverable

Create a short compatibility table in this document or `TRUTH.md`:

| Capability | macOS | Windows | Linux |
|---|---|---|---|
| Build app | proven / not proven | proven / not proven | proven / not proven |
| Global hotkey | works | missing / works | missing / works |
| Mic capture | works | unknown | unknown |
| Whisper sidecar | Apple Silicon only | missing | missing |
| Paste into focused app | works | missing | missing |
| Overlay | works | unknown | unknown |

### Proof Gate

Minimum proof for cross-platform readiness:

- `cargo check` or full Tauri build attempted on each target.
- All compile failures classified as either:
  - unsupported platform boundary missing,
  - missing sidecar asset,
  - missing system dependency,
  - real code bug.
- If not fixing Windows/Linux yet, app should fail clearly with a user-visible
  unsupported-platform message rather than silently pretending to work.

## Block 2 — Beta Packaging Matrix

Goal: define and verify the artifacts beta users will actually install.

### Platform Packaging Choices

macOS:

- Recommended beta artifact: signed + notarized DMG.
- Decide whether beta supports:
  - Apple Silicon only, or
  - universal / separate Intel and Apple Silicon builds.
- Current app has Apple Silicon whisper sidecar, so Intel support is not proven.

Windows:

- Recommended beta artifact: NSIS installer first, MSI only if there is a clear
  need.
- Expect Microsoft Defender SmartScreen friction without code signing.
- Need a Windows Whisper sidecar and any required DLLs.
- Confirm WebView2 availability story. Tauri relies on Microsoft Edge WebView2
  on Windows.

Linux:

- Recommended first beta artifact: AppImage.
- Add `.deb` later if users ask for it.
- Linux Tauri builds depend on WebKitGTK/appindicator-style system packages at
  build time and may have runtime integration differences by distro.
- Need a Linux Whisper sidecar and `.so` dependencies or a statically-linked
  strategy.

### Suggested Fixes / Prep

- Add a release matrix section to `README.md` before beta:
  - supported OS,
  - supported architectures,
  - install method,
  - known permissions,
  - known limitations.
- Add a `scripts/` or documented command list for building each artifact.
- Add a preflight script that checks required sidecar files exist before
  packaging.
- Consider naming artifacts clearly:
  - `TurboTalk-<version>-macos-arm64.dmg`
  - `TurboTalk-<version>-windows-x64-setup.exe`
  - `TurboTalk-<version>-linux-x64.AppImage`

### Tauri Notes

- Official Tauri prerequisite docs list different build dependencies by OS:
  <https://v2.tauri.app/start/prerequisites/>
- Official Tauri distribution docs cover platform artifact choices:
  <https://v2.tauri.app/distribute/>
- Official sidecar docs describe `externalBin` and target-triple-specific
  binaries:
  <https://tauri.app/develop/sidecar/>

### Proof Gate

For each supported beta platform:

- Fresh machine or clean VM install.
- App launches from the packaged artifact.
- User can grant required permissions.
- Model exists or can be downloaded.
- One short dictation lands in a normal text field.
- App quits/restarts cleanly.

## Block 3 — Stability And Failure-Mode Review

Goal: turn common beta breakages into clear, recoverable user experiences.

### Failure Modes To Exercise

Permissions:

- Mic permission denied.
- Accessibility/input permission denied.
- Clipboard unavailable or blocked.
- Autostart permission/LaunchAgent failure.

Audio:

- No input device.
- Selected device disappears.
- Bluetooth mic switches sample rate/channel count.
- Very short tap.
- Long recording with silence.
- Sleep/wake while app is running.

Transcription:

- Sidecar missing.
- Sidecar exists but is not executable.
- Sidecar exits non-zero.
- Model missing.
- Model corrupt or wrong format.
- Model path points outside allowed directory.
- Whisper output file missing.

Cleanup:

- Ollama not running.
- Ollama slow.
- Ollama returns malformed classifier output.
- Chaperone disabled / regex-only / raw mode.

Paste:

- Focus changes between recording and paste.
- Target app rejects paste.
- Clipboard contains rich/non-text content.
- Clipboard restore fails.

### Suggested Fixes / Prep

- Add a backend diagnostic command that returns status:
  - platform,
  - microphone availability,
  - selected model exists,
  - sidecar exists/executable,
  - cleanup mode and Ollama reachability,
  - paste capability status if detectable.
- Add a small diagnostics panel or copyable diagnostics text for beta reports.
- Keep current one-in-flight recorder invariant.
- Add precise UI messages for permission-denied cases. Avoid generic
  "transcription failed" when the root is actionable.
- Consider a "Test microphone" or "Record 2 second sample" button if beta users
  are likely to struggle with mic selection.

### Proof Gate

Create a manual beta smoke script with exact observations, for example:

1. Launch clean install.
2. Deny mic permission; verify app explains what to grant.
3. Grant mic; hold hotkey and speak "hello world"; verify transcript appears.
4. Remove/rename model; verify error points to model setup.
5. Enable Chaperone without Ollama; verify raw/regex fallback is clear.
6. Switch focus during transcription; verify banner names the destination.
7. Quit/relaunch; verify settings/history behavior matches config.

## Block 4 — Privacy And Beta Trust Review

Goal: make privacy behavior explicit enough for beta users to trust the app and
for bug reports to avoid leaking sensitive text.

### Current Privacy-Positive Behavior

- Default transcription is local Whisper sidecar.
- Chaperone is local Ollama only when enabled.
- Transcript body logging was redacted to character counts.
- History is local JSON under `~/.config/librewin/turbotalk/`.
- History has a retention setting.

### Remaining Decisions

- Should history default to on or off for beta?
- Should there be a "Never save history" mode distinct from "delete on
  restart"?
- Should beta diagnostics redact paths/usernames?
- Should model downloads be described as the only network call in default-ish
  setup?
- Should Advanced cleanup clearly say: "Sends transcript to your local Ollama
  server"?

### Suggested Fixes / Prep

- Add `PRIVACY.md` or a README privacy section:
  - what is recorded,
  - where audio temp files go,
  - when temp files are deleted,
  - where transcripts/history are stored,
  - when network calls happen,
  - what Chaperone sends to Ollama,
  - how to delete all local data.
- Add "Clear history" is already present; consider "Open data folder" or show
  the exact data path.
- Add "Do not save history" before public beta if testers may dictate sensitive
  text.
- Keep logs body-free. If debug transcript logging is ever reintroduced, gate it
  behind an explicit local debug flag and never enable it by default.

### Proof Gate

- A beta user can answer, from the app/README alone:
  - Does my voice leave my machine?
  - Does my transcript leave my machine?
  - Where is history stored?
  - How do I delete history/config/models?
  - What changes if I enable Advanced cleanup?

## Block 5 — Installer, Signing, Updates, And Release Operations

Goal: prevent the first beta from being blocked by operating-system trust
prompts, update confusion, or missing release process.

### Signing / Trust

macOS:

- Sign and notarize before external beta.
- Current local build signs ad-hoc (`signingIdentity: "-"`) and skips
  notarization. Good for local development, not ideal for beta.
- Document required permissions:
  - Microphone,
  - Accessibility/Input Monitoring if needed for hotkey/paste,
  - possibly Automation/System Events depending on paste implementation.

Windows:

- Code signing strongly recommended before broad beta.
- Unsigned installers can trigger SmartScreen warnings.
- If cross-compiling Windows installers from macOS/Linux, Tauri docs note that
  custom signing commands may be needed.

Linux:

- Signing expectations vary by artifact/repository.
- AppImage is simplest operationally, but desktop integration and permissions
  vary by distro/session.

### Updates

- For first beta, manual updates are acceptable if clearly documented.
- If enabling Tauri updater, updater artifacts must be signed; protect the
  private updater key. Losing it means existing installs cannot trust future
  updates from that key.
- Do not add auto-update until artifact naming, signing, and release hosting are
  stable.

Tauri updater signing reference:
<https://v2.tauri.app/plugin/updater/>

### Suggested Release Checklist

- Bump version in `package.json`, `src-tauri/Cargo.toml`, and
  `src-tauri/tauri.conf.json`.
- Build artifact for each supported platform.
- Run smoke test from installed artifact, not dev build.
- Produce checksums for downloadable artifacts.
- Publish release notes with:
  - supported platforms,
  - known limitations,
  - required permissions,
  - install/uninstall instructions,
  - data deletion instructions,
  - feedback channel.
- Archive proof in `SESSION-STATUS.md` or a small release note file.

### Proof Gate

For each released artifact:

- Artifact installs on a clean user account.
- App launches without developer tooling.
- Permissions flow is understandable.
- Dictation works once end-to-end.
- Upgrade or reinstall path is known.
- Uninstall path and local data cleanup path are documented.

## Suggested Agent Dispatch Order

1. **Compatibility Agent:** fill the OS capability table and add cfg boundaries
   where compilation is currently impossible.
2. **Packaging Agent:** define artifact matrix and verify sidecar naming/assets
   per target triple.
3. **Stability Agent:** build the failure-mode smoke script and improve the
   highest-friction error messages.
4. **Privacy Agent:** write `PRIVACY.md` / README privacy section and decide
   history default for beta.
5. **Release Ops Agent:** document signing, versioning, checksums, release notes,
   and manual update flow.

Each agent should leave one concrete proof statement. Example: "On macOS
arm64, the signed DMG installed into `/Applications`, requested microphone and
Accessibility permissions, and pasted 'hello world' into TextEdit."

