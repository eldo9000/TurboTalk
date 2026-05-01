# TurboTalk — Session Status

**Last updated:** 2026-05-01
**Current state:** M0 complete. Tauri 2 + Svelte 5 scaffold landed. Window opens with Libre chrome.

## Where We Are

Scaffold is live. `npm run tauri dev` opens a 480×400 window with the Libre titlebar
showing "TurboTalk" and the standard window controls. Theme and accent commands wired.

## Active Focus

M1 — hotkey capture + mic recording state machine.

## Blockers

None.

## Next Session Should

1. Implement `hotkey.rs` — register F1 as global push-to-talk via `tauri-plugin-global-shortcut`.
2. Implement `audio.rs` — open default mic via cpal, buffer 16kHz mono PCM.
3. Implement `recorder.rs` — 3-state machine (Ready → Recording → Transcribing → Ready).
4. Wire hotkey events → recorder state transitions via a Tauri event channel.
5. Proof: hold F1, speak, release; confirm WAV file written to tempdir.

## Recent Decisions

- **Reference, not fork.** Build from scratch. Reasoning in `ARCHITECTURE.md`.
- **Cargo patch for librewin-common.** `[patch]` in root `Cargo.toml` + `.cargo/config.toml`
  `net.git-fetch-with-cli = true` to resolve local path during dev.
- **Placeholder icons from Fade-App.** Real TurboTalk icons are M1+ work.
- **theme.rs command wrapper.** Added thin Tauri command wrappers for `get_theme` /
  `get_accent` — same pattern as Fade-App.
