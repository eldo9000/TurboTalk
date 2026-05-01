# TurboTalk Roadmap

Personal-use scope. Milestones are checkpoints, not deadlines.

## M0 — Block-out ✅

- [x] Repo structure decided
- [x] Architecture documented
- [x] Tauri 2 + Svelte 5 scaffold landed
- [x] `librewin-common` + `@libre/ui` wired
- [x] First `npm run tauri dev` succeeds

## M1 — End-to-end happy path ✅

The bar: press hotkey, speak, release, see text appear in the focused app.

- [x] Global hotkey capture — Right Alt via CGEventTap (rdev dropped: macOS 26 TSM thread crash)
- [x] Mic stream → WAV buffer — cpal, 24kHz mono F32, hound WAV writer
- [x] Whisper transcription — whisper-cli (Homebrew), ggml-base.en, ~130ms on M4 via Metal
- [x] Clipboard paste into active app — arboard + osascript Cmd+V, clipboard restored after
- [x] Recording overlay — dot: gray (idle) / red pulse (recording) / yellow pulse (transcribing)

Proved 2026-05-01. Spoken text lands in focused app in under 3 seconds.

## M2 — Configurable ✅

- [x] Tray icon — hide window, live in menu bar
- [x] Basic cleanup — capitalize first word, strip leading/trailing whitespace
- [x] Config persistence — `~/.config/librewin/turbotalk/config.toml`
- [x] Settings window — two-tab UI (History + Settings), whisper bin/model path
- [x] Whisper model selector / downloader hint — HuggingFace link + brew command

Proved 2026-05-01. Config persists across launches. Tray icon hides/shows window.

## M3 — Chaperone Layer

- [ ] Local LLM postprocessor wired (Ollama integration)
- [ ] Mode classifier (prose / code / command / raw)
- [ ] Per-mode deterministic handlers
- [ ] Voice commands ("scratch that", "new paragraph")

## M4 — Polish

- [ ] Codesigning + notarization (use Libre signing infra)
- [ ] Launch-on-login option
- [ ] Cross-platform paste (Windows + Linux)
- [ ] Streaming transcription (optional — big lift)
- [ ] Custom vocabulary / hotwords

## Open Questions

- Local LLM for Chaperone — Llama 3.2 3B or smaller? Latency budget ~200ms.
- Promote to Libre product if/when usable. Trigger: "I use this every day for 2 weeks."
