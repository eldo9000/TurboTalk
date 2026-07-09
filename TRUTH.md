# TurboTalk — Truth Ledger

What this project can honestly claim today. Updated when a claim changes.

**Operating model tier:** Tier 1 (small app, obvious behavior, personal-use scope). See `../Business-OS/standards/LIBRE-SOFTWARE-STANDARDS.md` §15.

---

## What works end-to-end

**Full dictation loop with persistent whisper-server** — confirmed 2026-05-08 (TASK-47):

1. Hold Right Alt → mic opens (<200ms), red dot pulses
2. Speak → audio captured 24 kHz mono F32 via cpal
3. Release Right Alt → WAV posted to persistent whisper-server (model stays loaded between dictations)
4. Transcript appears in TurboTalk window AND is pasted into the focused app via Cmd+V
5. Prior clipboard contents are restored after paste

Model warms at app startup via `prewarm()`. Second+ dictations skip the multi-second model reload.

**Long-recording timeout — fixed (commit `7238aa4`):** dictations whose
transcription ran longer than 30 s (≈ 3 min of dense speech) silently failed
with `"error sending request"` because the transcribe HTTP client inherited
reqwest's blocking-client default 30 s timeout. Now set to an explicit 120 s.

**Backend selector — supported: Whisper / Parakeet.** `BackendFamily` persists as `backend = "parakeet"` for new installs. Remnants of the retired Moonshine backend are still accepted on load via backward-compat deserialization and silently mapped to Parakeet. Recommended starter model per family: Parakeet TDT 0.6B v2, Whisper Large v3 Turbo.

**Parakeet backend — end-to-end confirmed 2026-05-26:** Full dictation loop works with int8 ONNX from `istupakov/parakeet-tdt-0.6b-v2-onnx` (~660 MB). Hold Right Alt → speak → release → transcript pasted. Output may be lowercase/unpunctuated (CTC architecture); Chaperone cleanup normalizes it.

**Moonshine backend — retired 2026-06-16:** Moonshine was previously validated end-to-end, but it is no longer a supported choice. Legacy configs still load and normalize to Parakeet, so existing installs keep working without manual edits.

**Silero VAD pre-filter — end-to-end confirmed 2026-05-26 (TASK-56):** `whisper.vad_enabled` (default true) passes `--vad --vad-model ggml-silero-v5.1.2.bin` to whisper-server so silent regions are skipped before decoding. User confirmed VAD trims leading/trailing silence on real mic input. Settings toggle in Modes tab → Whisper section. Model (~864 KB) bundled via `npm run fetch-vad-model`; ships in macOS release bundle. If absent at runtime, server starts without VAD (graceful fallback).

**Hallucination detection filter — landed 2026-05-23 (TASK-55):** three post-hoc signals (gzip compression ratio < 0.35, trigram repetition > 3×, non-letter ratio > 0.30) suppress Whisper garbage on silence; rejected transcripts shown with "⚠ filtered" badge, paste skipped.

**IOHID hotkey fallback — user-proven 2026-06-20:** IOHIDManager reads raw HID Button usage values at the IOKit level, bypassing CGEventTap entirely for mouse buttons. This works even when Logi Options+ (or similar driver software) intercepts the button — IOKit delivers HID reports to ALL registered IOHIDManager clients, so Logi cannot block TurboTalk from seeing the raw report. The same IOHIDManager callback now also handles Keyboard usage page 0x07 for Right Option/Control/Command/Shift and F13-F19 so ad-hoc macOS builds can use Input Monitoring instead of Accessibility. User-launched `/Applications/Turbo Talk.app` proof exists: two Right Option dictations started and stopped through IOHID keyboard events after restart, produced transcripts, and reached paste fallback.

**Ad-hoc macOS auto-paste via Session tap — user-proven 2026-06-21:** `CGEventPost(kCGSessionEventTap, Cmd+V)` works without `AXIsProcessTrusted()` on macOS Sequoia ad-hoc builds. When AX trust is false, TurboTalk now posts the keystroke at the Session tap level instead of the HID level, and leaves the transcript on the clipboard as a Cmd+V backup in case the OS drops the event. User confirmed auto-paste fires into Zed without any manual Cmd+V.

**Pause-media detection — user-proven 2026-07-08:** Pause-on-dictate no longer relies on MediaRemote/Now Playing or CoreAudio "is running" booleans. The installed app uses a CoreAudio process tap to sample actual system output energy before deciding whether to send Play/Pause, and leaves media alone if the tap is unavailable or silent. User confirmed it works. Logs prove active playback on the Bose Flex 2 output route produced `process tap result=1`, `rms=0.01482160`, `peak=0.16679211`, followed by `pause — playback detected, toggling`; after dictation completed, TurboTalk logged `resume — waiting 800ms then toggling`. Silent/no-playback probes still correctly return `result=0`.

**Hotkey modes — both hold and toggle confirmed 2026-06-16:** terminal launch now stays up, and the current hotkey flow works in real use with both press-and-hold and toggle-style operation.

**Terminal launch path — confirmed 2026-06-16:** `npm run tauri dev` now starts on `127.0.0.1:1431`, launches the Rust app, and stays alive. The earlier `::1:1428` bind failure was a localhost port conflict, not a broken app.

**Current hotkey startup state:** on the current macOS/ad-hoc build, `AXIsProcessTrusted()` returns false even when the app appears in Accessibility. The app treats Accessibility as granted for onboarding and relies on the IOHID/Input Monitoring keyboard fallback. Codex-launched tests are not authoritative for Input Monitoring because TCC attributes the request to responsible process `com.openai.codex` with requester `com.turbotalk.dictation`.

**Main window placement safeguards — patched 2026-06-13:** Main window minimum size is now 420×420 instead of 550×560. The frontend restores the preferred 550×560 utility size only when it fits the current monitor work area, and native code clamps the main window back into the visible work area on startup, first tray/menu show, focus, move, resize, and display-scale changes. Verified by `npm run typecheck`, `cargo check --manifest-path src-tauri/Cargo.toml`, and focused geometry unit tests. Runtime proof on the smaller laptop display is still pending.

**1.0 installed-artifact smoke — confirmed 2026-06-13:** User confirmed clean installed-artifact smoke is complete for the 1.0 path: macOS and Windows artifacts install, complete onboarding, dictate into a text target, quit/relaunch, and preserve settings/history.

**Parakeet vocab.txt SHA-256 fixed — patched 2026-06-13:** The `vocab.txt` hash was stale (HuggingFace updated the file); download failed SHA-256 verification during onboarding. Hash updated for both tdt-0.6b-v2 (`ec182b...`) and tdt-0.6b-v3 (`d58544...`). Compiled and verified against live HF content.

**Hotkey suppressed during onboarding — patched 2026-06-13:** PTT hotkey is now silently disabled while the welcome/onboarding screen is visible. Previously the hotkey fired during onboarding and produced error toasts ("start ignored — whisper prewarm failed earlier"). `ONBOARDING_ACTIVE` atomic flag gates `ptt_down`; cleared when readiness is immediately green at startup or by `clear_force_onboarding` when the user completes onboarding.

**Streaming chunked transcription — landed 2026-05-20 (TASK-54):** silence-boundary
segments emitted during recording are transcribed concurrently via `SegmentTranscriber`
so that by key-release only the final tail remains. Segments and tail assembled in
chronological order before the single cleanup pass. Batch fallback: if no segments
were emitted (short recordings, no silence boundary hit), path is identical to the
pre-TASK-54 whole-file POST.

Previously confirmed 2026-05-01 / 2026-05-03 with per-call `whisper-cli` spawn (M0–M5).

**Log-appender crash-on-init — fixed 2026-06-17:** `startup_logging::init()` no longer aborts when the filesystem denies creating rolling log files at `~/.config/turbotalk/logs/`. On macOS 26+, sandbox policy can return `EPERM` even when the directory exists (14 crashes on 2026-06-13 alone). The logging init is now best-effort: if any file appender fails, the app logs a warning to stderr and continues with console-only tracing. The dictation loop does not depend on file logging so this fallback is transparent. Transcript debug logging (`#[cfg(debug_assertions)]`) is also best-effort and never fatal.

## Pre-release scan sweep — 2026-06-02

Three pre-release scans run (read-only audits, reports in `docs/pre-release-scans/`):

- **Diagnostics / privacy — clean.** Dictated transcript text cannot reach an uploaded bug report. Isolation holds three ways: transcript text writes only to a dedicated `TRANSCRIPT_WRITER` sink (off the tracing pipeline), the report reader matches only the `turbotalk.*` log prefix (disjoint from `transcripts.*`), and the transcript writer is `#[cfg(debug_assertions)]`-gated so release builds never write it (`lib.rs:1906`). Live `transcript` IPC events carry text to the frontend but it stays in memory + local `history.json`; only char counts reach the UI-event log.
- **Beta bug report bundle — prepared 2026-06-13.** Settings → Developer → Report a bug now uses the shipping `submit_bug_report` path instead of the local-only export path. Each report saves a local `turbotalk-bugreport-<id>.txt` copy and opens the report folder via the `open` crate rather than raw `explorer`, fixing the known Windows folder-open failure path. Reports include build metadata, runtime paths, macOS version, history counts (not text), installed Whisper model inventory, Parakeet bundle validity, readiness, diagnostics, sanitized config, UI events, session logs, and WARN/ERROR logs. Transcript privacy boundary remains intact: dictated text is not included.
- **Packaging / update.** Not notarization-ready by design (ad-hoc `signingIdentity: "-"`). Updater was **blocked** — `createUpdaterArtifacts: false` suppressed the `.app.tar.gz` + `.sig` the release workflow hard-requires. **Fixed:** enabled in the CI release build only (via `--config` on the build step), keeping the committed config `false` so local `npm run package` stays DMG-only and needs no signing key. Needs a CI run to confirm end-to-end. macOS bundle codesign is now gated (`verify-macos-bundle.mjs`) and run in the release job; previously the only check was a `|| true` no-op. **Codesign gate verified locally 2026-06-02** — `npm run package` green, gate passes on the ad-hoc bundle, DMG + sha256 produced.
- **Regression — listeners + recovery.** Listener duplication **not present** (single `onMount` registration; one CGEventTap rebuilt only on Accessibility transitions; cpal stream is a single reused/replaced slot). Recovery signaling is honest at loss time (`device-lost` + `recording-discarded`, never a fake cancel). **One real bug fixed:** device-lost mid-hold in hold mode deferred a fake `recording-cancelled` onto the user's *next* press — the device-lost cancel bypassed `arm_ptt_up_suppression()`, so the trailing key-up hit IllegalTransition and set `CANCEL_PENDING`. Now armed in the `lib.rs:2286` device-lost block, mirroring the existing `trigger_cancel` callers. **Verified by construction (compiles, mirrors two verified call sites); runtime repro not yet observed.**

## v0.9.0 status — 2026-05-22

**Released.** Tagged and published at commit `4269aff`. First release confidently shareable with other Mac users — no known bugs on macOS. Direct-download link live in README (`TurboTalk-macOS-arm64.dmg` stable asset).

Marks end of macOS feature development. v0.9 → v1.0 arc is Windows/Linux porting only. No new features planned.

Bug fixes landed this session:
- Onboarding bounce during model download on window focus (`recheckReadiness` download-in-flight guard)
- Silent recording rejection / tail-empty segment-lost bug (`StopOutcome::Wav` speech_detected propagation)
- Whisper hallucination on `speech_detected=false` (tail Whisper call gated on flag) — **re-fixed 2026-05-29:** gate removed; trimmed WAV always transcribed, `detect_garbage` catches silence hallucinations
- Paste-miss false positive on Electron editors (Cursor/Zed) — **fixed 2026-05-29:** AX role no longer gates success; osascript Cmd+V success = paste ok
- Overlay stuck in recording mode during seg-recovery Whisper wait (emit `ptt-up` before `join_segments()`)
- New recording UI corrupted by delayed seg-recovery events (`CURRENT_JOB_ID` race check after join)
- Segment recovery (tail-too-short) leaks partial chunk text into history — emits `transcript` which frontend adds to `history.json`. Fixed: emit `recording-cancelled` instead in the segment recovery path so partial chunks are pasted but not persisted.

Model lineup: Recommended = `ggml-large-v3-turbo` (1.6 GB) · Small = `ggml-large-v3-turbo-q5_0` (574 MB) · Large = `ggml-large-v3` (3.1 GB).

## v0.8 beta packaging status — updated 2026-05-11

- `npm run package` builds the production frontend, release Rust binary,
  `Turbo Talk.app`, and macOS arm64 DMG.
- v0.8 intentionally skips Apple Developer credentials, so the beta is
  unsigned/ad-hoc and not notarized.
- Local `TurboTalk-0.8.12-macos-arm64.dmg.sha256` verifies. The `v0.8.12`
  tag points at commit `0b13130`; GitHub Actions release workflow is
  building the matrix (macos-arm64 + windows-x64). macOS is the
  usable beta path; Windows installs and UI runs but hotkey (rdev) and
  paste are unproven — rdev captured nothing in UTM/QEMU; real hardware test pending.
- A GitHub-downloaded macOS artifact is quarantined by macOS and may show
  "Apple could not verify Turbo Talk.app" on normal double-click. That is
  expected until Developer ID signing + notarization land; use right-click
  Open / Privacy & Security → Open Anyway for this beta.
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
- Config persists at `~/.config/turbotalk/config.toml` (TOML, written on first run)
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
- Recording overlay: always-on-top transparent window, 7-bar waveform animation, never steals focus. macOS placement now uses native AppKit screen/window coordinates and repositions before arming/recording events; user confirmed it appears on the monitor containing the mouse pointer.
- Overlay peek-through: cursor hover over recording pill dims background alpha + backdrop blur (confirmed 2026-05-04)

## What is hardcoded / not yet configurable

- Hotkey: Right Alt (Right Option) — not rebindable
- Cleanup mode defaults to regex; Chaperone requires Ollama running locally

## Windows platform plumbing — consistent 2026-06-10 (TASK-61)

Ten macOS-specific assumptions fixed. Not tested on Windows hardware, but no
longer macOS-only assumptions in source:

1. **Whisper stderr log** — uses `std::env::temp_dir()` instead of hardcoded `/tmp/`
2. **Child process env vars** — `TEMP` + `USERNAME` restored alongside `TMPDIR` + `USER`
3. **`kill_orphans()`** — `taskkill` on Windows, `pkill` on other platforms
4. **Config paths** — `dirs::config_dir()` returns `%APPDATA%` on Windows, `~/.config/` on macOS/Linux; one-time migration from legacy path
5. **Permission error messages** — platform-aware "System Settings" vs "Windows Settings"
6. **`open_data_folder()`** — `explorer` on Windows, `open` on macOS, `xdg-open` on Linux
7. **Sound chimes** — PowerShell `SystemSounds` on Windows (Hand, Asterisk, Exclamation)
8. **Window positioning** — consistent logical-coordinate math with explanatory comments
9. **Bundle config** — NSIS installer config added; `icon.ico` in icon array
10. **Microphone permission** — documented that `Unsupported` return is correct (cpal prompts naturally)

## What is explicitly not working

- Linux Whisper sidecar — upstream ships no Linux binary; Linux excluded from release matrix.
- Linux hotkey + paste — not validated on real X11 hardware; Linux is deferred to the 2.0 track.
- Developer ID codesigning / notarization and Windows Authenticode signing are intentionally deferred for 1.0.
- Developer ID codesigning / notarization and Windows Authenticode signing are intentionally deferred for 1.0 (separate from auto-paste, which now works via Session tap).

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
  preflight passes, `tauri build` produces an NSIS installer. Windows
  installed-artifact smoke is now complete per user confirmation.
- Linux: not bundled. Excluded from release matrix until rdev hotkey + paste
  validated on real X11 hardware.

## Key technical decisions

- **rdev dropped** — macOS 26 enforces `dispatch_assert_queue` on TSM APIs; rdev crashes
  on its background thread. Replaced with direct `CGEventTap` via `core-graphics 0.24`.
  Right Option detected by keycode 0x3D + `CGEventFlagAlternate` only — no TSM call.
- **IOHIDManager for hotkeys** — Mouse back/forward/middle hotkeys and the macOS
  ad-hoc keyboard fallback are handled by a second background thread running
  `IOHIDManager` with an input-value callback, rather than relying only on the
  CGEventTap. Mouse buttons use HID Button page 0x09; keyboard fallback uses HID
  Keyboard page 0x07. CGEventTap remains best-effort/legacy for signed builds where
  Accessibility trust is reliable.
- **Paste on ad-hoc macOS via Session tap** — Input Monitoring is enough to
  receive the Right Option hotkey through IOHID. When `AXIsProcessTrusted()` is
  false, TurboTalk posts Cmd+V at `kCGSessionEventTap` (below the Accessibility
  gate) rather than `kCGHIDEventTap`. The transcript is also left on the
  clipboard so a manual Cmd+V works if the OS drops the event. Auto-paste proven
  working on Sequoia ad-hoc build 2026-06-21. When AX trust is available (signed
  builds), HID-level injection is used instead.
- **Bundled whisper.cpp sidecar** — macOS arm64 sidecar is committed; Windows x64 sidecar is fetched in packaging from pinned upstream whisper.cpp v1.8.4; Linux sidecar is still absent.
- **Default model: ggml-large-v3-turbo** — 1.6 GB, multilingual, fast. Onboarding downloads it on first run; surfaced as "Recommended" in the Models tab. Earlier M0/M1 work used ggml-base.en (141 MB, ~130 ms on M4); the tiny model was rejected outright (stub weights in brew bundle).

## Tauri config rationale

- `macOSPrivateApi: true` (in `src-tauri/tauri.conf.json`) — required for
  `CGEventTap` hotkey monitoring (`hotkey.rs`). Removing it disables global
  push-to-talk.

## Promotion criteria

TurboTalk is a personal-use tool, not a public product. Promotion happens only if:
- Used daily for two consecutive weeks, AND
- Demonstrably works for at least one non-Eldo person, AND
- Chaperone Layer proves out and is worth shipping

## Known tradeoffs

- **Garbage detection (trigram repetition)** is a heuristic that catches
  hallucinations (repeating "lo lo lo...") but also fires on legitimate speech
  (stuttering, repeated S's, etc.). It no longer blocks paste or history — the
  `transcription-rejected` badge is just a UI warning.
