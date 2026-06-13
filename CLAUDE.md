# TurboTalk — Claude Context

## Shared Standards

- **Engineering standards:** `~/Downloads/Github/Business-OS/standards/ENGINEERING.md` — session protocol, investigation logs, commit conventions. Read before any implementation session.
- **Operating model:** `~/Downloads/Github/Business-OS/bin/SOFTWARE-DEVELOPMENT-OPERATING-MODEL.md` — the portfolio's evidence/ledger discipline. **TurboTalk operates at Tier 1** (see §15): small app, obvious behavior, personal-use scope. Required artifacts are limited to `SESSION-STATUS.md` (status ledger) and `TRUTH.md` (truth ledger). Do **not** add: heavy red-build ladders, observer loops, structured commit notes, milestone gates for every task, or full closure ceremony for every commit. Add weight only if a concrete failure mode appears.
- **Design language & shared patterns:** `~/Downloads/Github/Libre-Apps/CLAUDE.md` — design tokens, Tauri 2 patterns, Svelte 5 patterns, cross-app conventions. Read before any UI work.

## Tier 1 Habits (enforce these)

- Name the proof before calling work done. ("It compiles" is not proof. "I held F1, said 'hello world', and 'hello world' appeared in the focused TextEdit window" is proof.)
- Keep visible TODOs and stubs explicit — module headers in `src-tauri/src/*.rs` already do this.
- When a failure is not obvious, classify it before fixing.
- Update `SESSION-STATUS.md` after any meaningful work.
- Update `TRUTH.md` whenever the answer to "what works end-to-end" changes.

## What This Is

TurboTalk is a personal-use voice dictation utility for macOS. Push-to-talk hotkey → record mic → local Whisper transcription → optional LLM cleanup → paste into the focused app.

It is **not currently a Libre product.** Personal-use scope. If it earns its place, it gets promoted. Until then: private repo, GPL-3.0 license, no public release.

It consumes the Libre-Apps shared foundation (`librewin-common`, `@libre/ui`) but is otherwise standalone.

## Repo State

**v0.8 beta — working build, macOS arm64.** Full dictation loop proven end-to-end (2026-05-01). Milestones M0–M5 complete. The scaffold, all core modules, and the Chaperone guided-setup flow are all landed. See `TRUTH.md` for what works and `SESSION-STATUS.md` for current focus.

## Running

```bash
npm install
npm run tauri dev
```

Dev port: **1428**. For a packaged DMG: `npm run package` (produces `dist-artifacts/TurboTalk-<version>-macos-arm64.dmg`).

## Architecture

See `docs/ARCHITECTURE.md` for the full module plan. Key modules in `src-tauri/src/`:

- `audio.rs` — mic capture via `cpal`; keeps stream warm between recordings (45s idle-close watchdog)
- `recorder.rs` — 6-state dictation lifecycle (Ready / Recording / FinalizingAudio / Transcribing / Cleaning / Pasting)
- `transcribe.rs` — whisper.cpp sidecar wrapper; 300ms pre-roll ring buffer for leading-word preservation
- `paste.rs` — active-app text injection (arboard + osascript on macOS)
- `hotkey.rs` — global push-to-talk via CGEventTap (macOS); stub on other platforms
- `cleanup.rs` — LLM postprocessor (Chaperone Layer); emits `chaperone-fallback` ui-error toast on failure
- `ollama.rs` — Ollama HTTP helpers: `ping_ollama`, `check_ollama_model`, `open_url`, `pull_ollama_model`
- `settings.rs` — persistence under `~/.config/librewin/turbotalk/`; process-wide RwLock cache
- `diagnostics.rs` — health check command (Settings tab, dev-only surface)
- `whisper_models.rs` — model catalog, download command, progress events

## Portfolio Status

This repo participates in the Business-OS portfolio status system. Update `SESSION-STATUS.md` at the end of every session.

## Workflow

- macOS personal-use tool. No CI gates for now (add when Windows/Linux stubs are unblocked).
- The Chaperone Layer (classifier-router LLM via Ollama) is the differentiator. Reference `Business-OS/memory/project_chaperone_layer.md` for the pattern.
- Promote to Libre product trigger: "I use this every day for 2 weeks."
