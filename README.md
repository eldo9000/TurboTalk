# TurboTalk

Personal voice dictation for macOS. Hold a key → speak → text pastes into whatever app has focus. Runs entirely on-device.

## Requirements

- macOS (Apple Silicon / arm64)
- **Accessibility permission** — System Settings → Privacy & Security → Accessibility → TurboTalk (required for the global hotkey and paste injection)
- A whisper model file (see below)

## Getting Started

1. **Download a model.** Drop a `.bin` file from [ggerganov/whisper.cpp](https://github.com/ggerganov/whisper.cpp/tree/master/models) into:
   ```
   ~/.config/librewin/turbotalk/models/
   ```
   `ggml-small.en.bin` is a good starting point (~150 MB, fast on Apple Silicon).

2. **Launch TurboTalk.** The app lives in the menu bar.

3. **Grant Accessibility.** On first use macOS will prompt — or go to System Settings → Privacy & Security → Accessibility and add TurboTalk manually. Restart the app after granting.

4. **Hold your hotkey and speak.** Default is **Right Option ⌥**. Release to transcribe. The text pastes automatically.

## Features

- **Global push-to-talk** — hold the hotkey to record, release to transcribe and paste. Or switch to toggle mode (press once to start, press again to stop).
- **Configurable hotkey** — choose from Right Option ⌥, Right Control ⌃, Right Command ⌘, or Right Shift ⇧.
- **Local Whisper transcription** — whisper.cpp sidecar, Metal-accelerated on Apple Silicon. No internet required.
- **Recording overlay** — a small floating pill animates while you speak and while the model transcribes, then disappears.
- **Optional LLM cleanup** — connect an Ollama endpoint to run a postprocessor that fixes punctuation, capitalisation, and formatting (Chaperone Layer).
- **Transcript history** — recent dictations shown in the main window.
- **Theme** — auto (follows macOS), light, or dark.
- **Zoom** — scale the UI from 80% to 140%.
- **Launch at login** — toggle in Settings.

## No Cloud, No Telemetry

Everything runs locally. No account, no network calls, no data leaves your machine.

## Building from Source

```bash
# Prerequisites: Rust (stable), Node 20+, Xcode Command Line Tools
npm install
npm run tauri dev
```

The whisper sidecar binary is bundled in `src-tauri/binaries/`. If you want to swap it for a locally compiled build, update the path under **Settings → Advanced**.

## License

MIT.
