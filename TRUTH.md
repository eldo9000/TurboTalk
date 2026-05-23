# TurboTalk — Truth Ledger

What this project can honestly claim today. Updated when a claim changes.

**Operating model tier:** Tier 1 (small app, obvious behavior, personal-use scope). See `~/Downloads/Github/Business-OS/bin/SOFTWARE-DEVELOPMENT-OPERATING-MODEL.md` §15.

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

**Silero VAD pre-filter — wired 2026-05-23 (TASK-56):** `whisper.vad_enabled` (default true) passes `--vad --vad-model ggml-silero-v5.1.2.bin` to whisper-server so silent regions are skipped before decoding. Settings toggle in Modes tab → Whisper section. VAD model placeholder in `src-tauri/binaries/` — replace with the real 2 MB model from `https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin`. If absent, server starts without VAD (graceful fallback).

**Hallucination detection filter — landed 2026-05-23 (TASK-55):** three post-hoc signals (gzip compression ratio < 0.35, trigram repetition > 3×, non-letter ratio > 0.30) suppress Whisper garbage on silence; rejected transcripts shown with "⚠ filtered" badge, paste skipped.

**Streaming chunked transcription — landed 2026-05-20 (TASK-54):** silence-boundary
segments emitted during recording are transcribed concurrently via `SegmentTranscriber`
so that by key-release only the final tail remains. Segments and tail assembled in
chronological order before the single cleanup pass. Batch fallback: if no segments
were emitted (short recordings, no silence boundary hit), path is identical to the
pre-TASK-54 whole-file POST.

Previously confirmed 2026-05-01 / 2026-05-03 with per-call `whisper-cli` spawn (M0–M5).

## v0.9.0 status — 2026-05-22

**Released.** Tagged and published at commit `4269aff`. First release confidently shareable with other Mac users — no known bugs on macOS. Direct-download link live in README (`TurboTalk-macOS-arm64.dmg` stable asset).

Marks end of macOS feature development. v0.9 → v1.0 arc is Windows/Linux porting only. No new features planned.

Bug fixes landed this session:
- Onboarding bounce during model download on window focus (`recheckReadiness` download-in-flight guard)
- Silent recording rejection / tail-empty segment-lost bug (`StopOutcome::Wav` speech_detected propagation)
- Whisper hallucination on `speech_detected=false` (tail Whisper call gated on flag)
- Overlay stuck in recording mode during seg-recovery Whisper wait (emit `ptt-up` before `join_segments()`)
- New recording UI corrupted by delayed seg-recovery events (`CURRENT_JOB_ID` race check after join)

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

- Windows hotkey — `rdev` `WH_KEYBOARD_LL` hook installs but captures no events in UTM/QEMU (x64-emulated on ARM64 + virtio keyboard bypasses Win32 low-level hook chain). Untested on real Windows hardware. TASK-25.
- Windows paste — unreachable without working hotkey; arboard+enigo impl in place, untested. TASK-26.
- Windows tray icon — renders as transparent/invisible blue square; no TT glyph. Cosmetic.
- Windows onboarding flag — welcome screen re-triggers on every restart; "onboarding complete" state not persisting correctly on Windows config path.
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
- **Bundled whisper.cpp sidecar** — macOS arm64 sidecar is committed; Windows x64 sidecar is fetched in packaging from pinned upstream whisper.cpp v1.8.4; Linux sidecar is still absent.
- **Default model: ggml-large-v3-turbo** — 1.6 GB, multilingual, fast. Onboarding downloads it on first run; surfaced as "Recommended" in the Models tab. Earlier M0/M1 work used ggml-base.en (141 MB, ~130 ms on M4); the tiny model was rejected outright (stub weights in brew bundle).

## Tauri config rationale

- `macOSPrivateApi: true` (in `src-tauri/tauri.conf.json`) — required for
  `CGEventTap` hotkey monitoring (`hotkey.rs`). Removing it disables global
  push-to-talk.

## Promotion criteria

TurboTalk is a personal-use tool, not a Libre product. Promotion happens only if:
- Used daily for two consecutive weeks, AND
- Demonstrably works for at least one non-Eldo person, AND
- Chaperone Layer proves out and is worth shipping
