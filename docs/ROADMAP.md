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
- [x] Mic stream → WAV buffer — cpal native-rate F32 capture, FFT-resampled + downmixed to 16kHz mono int16 before WAV write
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
- [x] Next-beta hardening — scratch-that discards instead of pasting, cancel can abort an in-flight Whisper child, paste failure restores clipboard, device loss drops the warm stream, diagnostics only probes loopback Ollama URLs, onboarding/settings handle unsupported platforms honestly, and Settings window sizing/hotkey labels received final UI polish (v0.8.6 tag, 2026-05-06)

Proved 2026-05-06 at code/build level: `cargo test --manifest-path src-tauri/Cargo.toml` passed (80 passed, 1 ignored real-audio VAD test), `npm run typecheck` passed, `npm run build` passed with known Svelte/shared warnings, and local `TurboTalk-0.8.6-macos-arm64.dmg.sha256` verified. Installed-artifact smoke test from the GitHub release build remains the final beta-publication gate.

## M6 — Ship (1.0)

The bar: TurboTalk installs cleanly on macOS / Windows / Linux without Gatekeeper or SmartScreen warnings, and the dictation loop works end-to-end on each.

- [x] Publish v0.8.6 beta prerelease — tag `v0.8.6` points at `352a251`; release workflow run 25414134047 is green and the prerelease is live
- [ ] Run installed-artifact smoke against the published v0.8.6 macOS artifact on a clean macOS account
- [ ] Developer ID notarized macOS beta — downloaded GitHub DMGs are quarantined by macOS and blocked unless the user right-clicks Open/Open Anyway or clears quarantine; real fix is Developer ID signing + notarization
- [ ] On-device latency proof — verify v0.8.6 warm-stream + pre-roll fix end-to-end on built-in mic and Bluetooth (AirPods); confirm leading-word capture and ~10ms PTT-to-capture
- [ ] Cross-platform hotkey + paste — Windows (`enigo`/SendInput) and Linux (xdotool / wl-clipboard); off-mac runtime still returns unsupported for the real dictation loop (TASK-25/26 deferred)
- [ ] Cross-platform diagnostics + onboarding — Win/Linux readiness gates (mic permission, sidecar present, Accessibility/equivalent), per-OS permission flow (TASK-29 deferred)
- [ ] Linux release pipeline — re-enable Linux in `.github/workflows/release.yml` matrix once the runtime path (rdev under X11/Wayland, AppImage system deps) has been validated on real hardware
- [ ] Codesigning + notarization — Apple Developer ID for macOS, Authenticode for Windows; use Libre signing infra
- [ ] Run-on-real-hardware proof — at least one verified install + dictation loop on each platform from a fresh DMG / MSI / AppImage (no dev environment dependencies)

## Out of scope for 1.0

- **Streaming transcription** (partial transcripts while recording) — significant DSP/UX lift and does not change the "press → speak → paste" quality bar. Revisit post-1.0 only if a concrete user need shows up.
- **Public release / Libre product status** — TurboTalk stays personal-use under GPL-3.0 until the promotion trigger fires.

## Open Questions

- Promote to Libre product if/when usable. Trigger: "I use this every day for 2 weeks."
