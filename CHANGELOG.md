# Changelog

## v0.8.12 — 2026-05-11

### Onboarding

- Full guided onboarding flow: Input Monitoring → Microphone → Accessibility → model download → Launch at Login
- Sequenced permission prompts with status polling and deep-links to System Settings
- IOHIDManager pre-registration at startup so the app appears in Input Monitoring before the user opens System Settings
- "Reset Turbo Talk" option to re-run onboarding from Settings
- Onboarding lets you select an already-downloaded model instead of forcing a re-download

### Fixes

- Gate macOS hotkey on Input Monitoring status; wait for registration before proceeding
- Collapse repeated whitespace/newlines from Whisper segment output
- Default recording-length overlay is now off
- Resize onboarding window to content; compact completed steps; allow scrolling on small screens
- Input Monitoring permission request now uses `CGRequestListenEventAccess` (CoreGraphics, macOS 12+) as the primary path with IOKit as fallback

### Docs

- README: clarify update behavior (manual check-for-updates button in Settings, not auto-on-launch)
- README: document the Accessibility re-add workaround for the first install on macOS
- RELEASING.md: accurately describe the update policy (user-initiated check, not background auto-update)

---

## v0.8.11 — 2026-05-09

- Fix process leak: whisper-server orphan cleanup on app exit via `RunEvent::Exit`
- Fix triple-paste / repetition hallucination: restore whisper-server decoding flags (temperature=0, suppress_nst=true, beam_size=5)
- Fix long-utterance cutoff: dynamic audio_ctx selection — 512 for ≤8s, 0 (full 30s) for longer
- Shelve CoreML / Neural Engine phase 2 (dyld-init ANE-warmup blocker unresolved)

## v0.8.10 — 2026-05-08

- TASK-47: persistent whisper-server worker replaces per-call whisper-cli spawn
- Model warms at app startup; second+ dictations skip model reload
- Fix GGML_ASSERT crash: search binaries/ before target/debug/ for whisper-server to avoid stale dylib conflict

## v0.8.9 — 2026-05-07

- Multi-monitor overlay fix: recording pill tracks the display containing the focused window
- Whisper non-speech token strip (removes `[BLANK_AUDIO]`, `(silence)`, etc.)

## v0.8.8 — 2026-05-06

- LSUIElement / agent-style main window (no Dock icon)

## v0.8.7 — 2026-05-06

- Overlay peek-through: cursor hover dims recording pill without stealing focus
- Push-to-talk overlay fix for multi-monitor setups

## v0.8.0 — 2026-05-03

Initial macOS arm64 beta. Full dictation loop: Right Alt → record → whisper transcription → paste into focused app.
