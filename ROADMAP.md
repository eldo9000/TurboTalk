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

## M3 — Chaperone Layer ✅

- [x] Local LLM postprocessor wired (Ollama integration, blocking reqwest)
- [x] Mode classifier (prose / code / command / raw) via Ollama
- [x] Per-mode deterministic handlers
- [x] Voice commands ("scratch that", "new paragraph")

Proved 2026-05-01. Chaperone routes transcripts through local LLM; falls back to prose on error.

## M4 — Polish ✅

- [x] Launch-on-login (tauri-plugin-autostart, LaunchAgent)
- [x] Mic selector (list_audio_devices command, settings UI)
- [x] Dynamic tray icon — TT glyph idle / red dot recording / amber dot transcribing
- [x] Zoom controls — 9 levels (100–180%), keyboard shortcuts (⌘+/⌘-/⌘0), persistent
- [x] Three-tab UI — History / Models / Modes / Settings with auto-fit window sizing per tab
- [x] Recording overlay — always-on-top transparent waveform + transcript size indicator (word-pill accumulator)
- [x] Models tab — active model selector, installed list, HuggingFace download catalog
- [x] Whisper bundled as Tauri sidecar (mac arm64 committed; Win x64 fetched via `npm run fetch-sidecars`, pinned to whisper.cpp v1.8.4)
- [x] Custom vocabulary / hotwords (`cleanup.vocabulary` → whisper `--prompt`; surfaced in Modes tab)
- [x] Audio sound indicators UI — per-event checkboxes (start / transcribe / finish) + volume slider in Settings
- [x] Cancel on Escape — `cancel_on_esc` config, `recording-cancelled-tap` event, overlay handles gracefully

Proved 2026-05-05.

## M5 — Chaperone Setup + Reliability ✅

The bar: Chaperone mode is discoverable and self-configuring for a first-time user.

- [x] Guided Ollama setup in Modes → Advanced — detects Ollama reachability and model pull status, "Install Ollama" browser-open button, "Download classifier model" streaming pull with progress bar, green "Ready" pill when both gates pass (TASK-32–34, 2026-05-05)
- [x] Chaperone fallback ui-error toast — when Ollama is unreachable during dictation, fires a rate-limited (60s) recoverable toast "Chaperone unreachable — used raw output. Set up Ollama in Modes → Advanced."; click switches to Modes tab (TASK-35, 2026-05-05)
- [x] Audio-latency improvements — cpal stream warm-keep with 45s idle-close watchdog, 300ms pre-roll ring buffer so leading words aren't clipped, config RwLock cache so PTT-down skips per-press file I/O; PTT capture now ~10ms vs. prior 50–500ms (TASK-36–38, 2026-05-05)

Proved 2026-05-05 (build + TS check clean; on-device latency verification by user pending).

## M6 — Ship

- [ ] Codesigning + notarization (use Libre signing infra)
- [ ] Cross-platform paste — Windows (SendInput) + Linux (xdotool) — TASK-25/26 deferred
- [ ] Streaming transcription (optional — big lift)
- [ ] Promote to Libre product trigger: "I use this every day for 2 weeks."

## Open Questions

- Promote to Libre product if/when usable. Trigger: "I use this every day for 2 weeks."
