# TurboTalk — Truth Ledger

What this project can honestly claim today. Updated when a claim changes.

**Operating model tier:** Tier 1 (small app, obvious behavior, personal-use scope). See `~/Downloads/Github/Business-OS/bin/SOFTWARE-DEVELOPMENT-OPERATING-MODEL.md` §15.

---

## What works end-to-end

**Full dictation loop** — confirmed 2026-05-01 on macOS 26.4.1 (Apple M4);
confirmed again by user during v0.8 beta prep on 2026-05-03:

1. Hold Right Alt → mic opens (<200ms), red dot pulses
2. Speak → audio captured 24 kHz mono F32 via cpal
3. Release Right Alt → whisper-cli runs ggml-base.en via Metal (~130ms on M4)
4. Transcript appears in TurboTalk window AND is pasted into the focused app via Cmd+V
5. Prior clipboard contents are restored after paste

Proof: `[audio] wrote 42240 samples` (1.76s voice), transcript landed in Notes.app.

## v0.8 beta packaging status — confirmed 2026-05-04

- `npm run package` builds the production frontend, release Rust binary,
  `Turbo Talk.app`, and macOS arm64 DMG.
- v0.8 intentionally skips Apple Developer credentials, so the beta is
  ad-hoc signed and not notarized.
- The final canonical artifact exists at
  `dist-artifacts/TurboTalk-0.8.0-macos-arm64.dmg` with a matching `.sha256`;
  `shasum -a 256 -c` passes when run from `dist-artifacts/`.
- Packaged app diagnostics confirmed microphone input works, model exists, and
  the bundled sidecar resolves from
  `/Applications/Turbo Talk.app/Contents/MacOS/whisper-cli`.
- Packaged global hotkey works after the installed app is manually removed and
  re-added in macOS Accessibility settings.
- Temporary beta troubleshooting controls were removed after proof: no
  production-visible manual record button, copied mic probe details, or debug
  log command remain.

## M2 also working — confirmed 2026-05-01

- Tray icon: left-click shows/hides window; right-click menu has Show + Quit
- Close button hides to tray (does not quit)
- Config persists at `~/.config/librewin/turbotalk/config.toml` (TOML, written on first run)
- Settings tab: whisper bin + model path editable and saved live
- History tab: last 50 transcripts shown, most recent first
- Whisper model hint: HuggingFace link opens in browser; brew command shown

## M3 + M4 (partial) also working — confirmed 2026-05-01

- Chaperone Layer: Ollama classifier routes to prose/code/command/raw handlers; voice commands ("scratch that", "new paragraph") bypass LLM
- Launch at login: toggle in Settings tab (tauri-plugin-autostart, LaunchAgent)
- Mic selector: dropdown in Settings tab; restart required to apply
- Dynamic tray icon: TT glyph (idle) / red dot (recording) / amber dot (transcribing)
- Zoom controls: 9 levels 100–180%, ⌘+/⌘-/⌘0, persistent in localStorage
- Models tab: active model selector, installed model list with add/remove, HuggingFace catalog with download links
- Recording overlay: always-on-top transparent window, 7-bar waveform animation, never steals focus
- Overlay peek-through: cursor hover over recording pill dims background alpha + backdrop blur (confirmed 2026-05-04)

## What is hardcoded / not yet configurable

- Hotkey: Right Alt (Right Option) — not rebindable
- Cleanup mode defaults to regex; Chaperone requires Ollama running locally

## What is explicitly not working

- Windows hotkey + paste — stubs only (`Err("unsupported platform")`); TASK-25/26.
- Linux Whisper sidecar — upstream ships no Linux binary; Linux excluded from release matrix.
- Developer ID codesigning / notarization is intentionally deferred for v0.8.

## Sidecar bundling — confirmed 2026-05-05

- macOS arm64: `whisper-cli` + 3 dylibs committed under `src-tauri/binaries/`.
  Tauri auto-copies them into `Contents/Resources/` (`@executable_path/../Resources`
  rpath resolves at runtime). Verified via `otool -L`.
- Windows x64: `npm run fetch-sidecars` downloads upstream
  `whisper-bin-x64.zip` (whisper.cpp v1.8.4, sha256-pinned) and extracts
  `whisper-cli.exe` + 4 DLLs into `src-tauri/binaries/`. DLLs declared in
  `src-tauri/tauri.windows.conf.json` `bundle.resources`. CI release
  workflow ran green on commit `0e9ad71`
  (https://github.com/eldo9000/TurboTalk-App/actions/runs/25378189425) —
  preflight passes, `tauri build` produces an NSIS installer. Runtime
  proof on a real Windows box still pending (hotkey + paste are stubs).
- Linux: not bundled. Excluded from release matrix until rdev hotkey + paste
  validated on real X11 hardware.

## Key technical decisions

- **rdev dropped** — macOS 26 enforces `dispatch_assert_queue` on TSM APIs; rdev crashes
  on its background thread. Replaced with direct `CGEventTap` via `core-graphics 0.24`.
  Right Option detected by keycode 0x3D + `CGEventFlagAlternate` only — no TSM call.
- **Homebrew whisper-cpp** — Metal-accelerated, not bundled as Tauri sidecar yet (M2).
- **ggml-base.en** — 141MB, ~130ms latency on M4. Tiny model rejected (stub weights in brew bundle).

## Tauri config rationale

- `macOSPrivateApi: true` (in `src-tauri/tauri.conf.json`) — required for
  `CGEventTap` hotkey monitoring (`hotkey.rs`). Removing it disables global
  push-to-talk.

## Promotion criteria

TurboTalk is a personal-use tool, not a Libre product. Promotion happens only if:
- Used daily for two consecutive weeks, AND
- Demonstrably works for at least one non-Eldo person, AND
- Chaperone Layer proves out and is worth shipping
