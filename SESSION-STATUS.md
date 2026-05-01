# TurboTalk — Session Status

**Last updated:** 2026-05-01
**Current state:** M1 complete. Full dictation loop proven end-to-end.

## Where We Are

Roadmap M0 and M1 are done. Core loop works:
Right Alt → record → whisper transcription → paste into focused app.

Commits this session:
- `5063e13` scaffold: Tauri 2 + Svelte 5 + librewin foundation
- `12726ba` feat: hotkey + mic capture (CGEventTap, cpal, hound)
- `0d5d5be` feat: whisper transcription (whisper-cli, ggml-base.en)
- `6557ed8` feat: paste into focused app (arboard + osascript)
- `0c00ece` fix: UI chrome + transcribing state reset

## Active Focus

Roadmap M2 — Configurable.

## Blockers

None.

## Next Session Should

1. Tray icon — hide window to menu bar, show/hide on click.
2. Basic text cleanup — capitalize first word, strip trailing whitespace.
3. Config persistence — `~/.config/librewin/turbotalk/config.toml` via `settings.rs`.
4. Settings window — surface model path and hotkey to user.

## Recent Decisions

- **rdev → CGEventTap** — macOS 26 broke rdev (TSM `dispatch_assert_queue` crash). Direct
  `CGEventTap` via `core-graphics 0.24`. Right Option detected by keycode 0x3D only, no TSM.
- **Homebrew whisper-cpp** — not bundled as Tauri sidecar yet; hardcoded path for now.
- **ggml-base.en** — 141MB, ~130ms on M4 via Metal. Adequate for M1.
- **Window: 380×280** — no custom titlebar, native macOS traffic lights only.
- **Reference, not fork** — built from scratch. Handy/typr/sagascript as references.
