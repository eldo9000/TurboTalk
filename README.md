# TurboTalk

Personal voice dictation utility for macOS. Push-to-talk → local Whisper → paste into any app.

**Status:** Block-out. Architecture decided, Tauri scaffolding pending.

## What It Is

A small, fast, local-first dictation tool. Hotkey-triggered, headless most of the time, with a settings window and a recording overlay when active. No cloud, no telemetry, no account.

## Stack

- **Tauri 2** — window + tray + IPC
- **Rust** — backend (mic capture, whisper.cpp, hotkey, paste injection)
- **Svelte 5** — settings UI + overlay
- **whisper.cpp** — local STT (sidecar binary, Metal acceleration on Apple Silicon)
- **Local LLM** (Ollama or llama.cpp) — optional postprocessor for punctuation, formatting, and command/prose classification (Chaperone Layer pattern)
- **`librewin-common` + `@libre/ui`** — shared Libre foundation (theme, components, design tokens)

## Why Not Just Fork Handy / typr

Decided to reference, not fork. Smaller surface, full ownership, room to bake in the Chaperone Layer pattern from day one. See `ARCHITECTURE.md` for module plan and reference repos.

## License

MIT.
