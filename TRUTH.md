# TurboTalk — Truth Ledger

What this project can honestly claim today. Updated when a claim changes.

**Operating model tier:** Tier 1 (small app, obvious behavior, personal-use scope). See `~/Downloads/Github/Business-OS/bin/SOFTWARE-DEVELOPMENT-OPERATING-MODEL.md` §15.

---

## What works end-to-end

**M0 scaffold** — `npm run tauri dev` opens a window with Libre chrome (titlebar, window
controls, theme). Confirmed visually on 2026-05-01. The window shows "TurboTalk" in the
titlebar and "Voice dictation — M0 scaffold" in the body.

## What is partially proven

- Rust compilation: all 451 crates compile clean on macOS.
- Frontend: Vite + Svelte 5 + Tailwind 4 build and serve at port 1428.
- `@libre/ui` components (WindowFrame, Titlebar) render correctly.
- `get_theme` / `get_accent` Tauri commands registered and resolving.

## What is explicitly not working

- Hotkey capture (not implemented)
- Mic recording (not implemented)
- Whisper transcription (not implemented)
- LLM cleanup / Chaperone Layer (not implemented)
- Text paste injection (not implemented)
- Settings persistence (not implemented)

## Support claims that are FORBIDDEN until stronger evidence exists

- "TurboTalk transcribes voice." (No transcription code exists.)
- "Push-to-talk works." (No hotkey code exists.)
- "It pastes into apps." (No paste code exists.)
- Any claim about latency, accuracy, or quality.

## Current strategic bet

**Reference, not fork.** Build from scratch using Handy / typr / sagascript as references.
Reversal trigger: if M1 (hotkey + audio + recorder) takes more than two sessions, fork Handy instead.

## Promotion criteria

TurboTalk is a personal-use tool, not a Libre product. Promotion happens only if:
- Eldo uses it daily for two consecutive weeks, AND
- It demonstrably works for at least one non-Eldo person, AND
- The Chaperone Layer pattern proves out and is worth shipping
