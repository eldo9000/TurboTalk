# TurboTalk — Claude Context

## Shared Standards

- **Engineering standards:** `~/Downloads/Github/Business-OS/standards/ENGINEERING.md` — session protocol, investigation logs, commit conventions. Read before any implementation session.
- **Design language & shared patterns:** `~/Downloads/Github/Libre-Apps/CLAUDE.md` — design tokens, Tauri 2 patterns, Svelte 5 patterns, cross-app conventions. Read before any UI work.

## What This Is

TurboTalk is a personal-use voice dictation utility for macOS. Push-to-talk hotkey → record mic → local Whisper transcription → optional LLM cleanup → paste into the focused app.

It is **not currently a Libre product.** Personal-use scope. If it earns its place, it gets promoted. Until then: private repo, MIT license, no public release.

It consumes the Libre-Apps shared foundation (`librewin-common`, `@libre/ui`) but is otherwise standalone.

## Repo State

**Block-out only.** No working build yet. The architecture and module layout are decided; the Tauri scaffolding has not been generated. To start coding:

1. Run `npm create tauri-app@latest` in a temp dir to generate `tauri.conf.json`, `build.rs`, `capabilities/`, and the Vite frontend skeleton.
2. Copy those files into this repo. Reconcile with the existing `Cargo.toml` / `package.json`.
3. Vendor `~/Downloads/Github/Libre-Apps/common-js/` into `./common-js/` for `@libre/ui`.
4. `npm install && npm run tauri dev`.

Do not commit a half-broken scaffold. Land it as one clean "scaffold Tauri 2 + Svelte 5" commit.

## Architecture

See `ARCHITECTURE.md` for the module plan. Key modules in `src-tauri/src/`:

- `audio.rs` — mic capture via `cpal`
- `recorder.rs` — 3-state machine (Ready / Recording / Transcribing)
- `transcribe.rs` — whisper.cpp sidecar wrapper
- `paste.rs` — active-app text injection (osascript on macOS)
- `hotkey.rs` — global push-to-talk via `tauri-apps/global-hotkey`
- `cleanup.rs` — LLM postprocessor (Chaperone Layer)
- `settings.rs` — persistence under `~/.config/librewin/turbotalk/`

## Reference Repos (do not fork — read and learn from)

- **Handy** (`cjpais/Handy`) — Rust + Tauri, MIT, 20k★, production. Closest match to what we're building.
- **typr** (`albertshiney/typr`) — Rust + Tauri, vibe-coded, 8 clean modules, no license. Good module-layout reference.
- **sagascript** (`Magnus-Gille/sagascript`) — Rust, tiny, idiomatic macOS-glue patterns.
- **whisper.cpp** (`ggerganov/whisper.cpp`) — STT engine, ship as sidecar.

## Running (when scaffolding is done)

```bash
npm install
npm run tauri dev
```

Dev port: **1428** (Fade uses 1427, increment per Libre app).

## Portfolio Status

This repo participates in the Business-OS portfolio status system. Update `SESSION-STATUS.md` at the end of every session.

## Workflow

- Personal-use tool — no CI gates required initially. Add CI when first usable build lands.
- Mission: get to "I can dictate this paragraph" as fast as possible. Polish later.
- The Chaperone Layer (classifier-router LLM) is the differentiator. Reference `Business-OS/memory/project_chaperone_layer.md` for the pattern.
