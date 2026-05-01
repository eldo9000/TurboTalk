# TurboTalk Roadmap

Personal-use scope. Milestones are checkpoints, not deadlines.

## M0 — Block-out (current)

- [x] Repo structure decided
- [x] Architecture documented
- [ ] Tauri 2 + Svelte 5 scaffold landed
- [ ] `librewin-common` + `@libre/ui` wired
- [ ] First `npm run tauri dev` succeeds

## M1 — End-to-end happy path

The bar: press hotkey, speak, release, see text appear in the focused app.

- [ ] Global hotkey capture (push-to-talk)
- [ ] Mic stream → WAV buffer
- [ ] Whisper.cpp sidecar with `small` model
- [ ] Clipboard paste into active app (macOS only)
- [ ] Recording overlay (just a dot — "ready / listening / thinking")

No settings UI yet. No cleanup. Hardcoded everything. Goal: prove the loop works.

## M2 — Configurable

- [ ] Settings window (model select, hotkey rebind, mic select)
- [ ] Config persistence under `~/.config/librewin/turbotalk/`
- [ ] Tray icon
- [ ] Whisper model downloader UI
- [ ] Basic regex cleanup (capitalize first letter, period at end)

## M3 — Chaperone Layer

- [ ] Local LLM postprocessor wired (Ollama integration)
- [ ] Mode classifier (prose / code / command / raw)
- [ ] Per-mode deterministic handlers
- [ ] Voice commands ("scratch that", "new paragraph")

## M4 — Polish

- [ ] Codesigning + notarization (use Libre signing infra)
- [ ] Cross-platform (Windows + Linux paste injection)
- [ ] Streaming transcription (optional — big lift)
- [ ] Custom vocabulary

## Open Questions

- Whisper model default — `small` or `base.en`? Test latency on Apple Silicon.
- Local LLM model for Chaperone — Llama 3.2 3B or smaller? Latency budget is ~200ms.
- Should TurboTalk launch on login, or be opened manually each session? (Login agent recommended.)
- Promote to Libre product if/when usable. Trigger: "I use this every day for 2 weeks."
