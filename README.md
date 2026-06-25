<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="Turbo Talk" />
</p>

<h1 align="center">Turbo Talk</h1>


<p align="center">
  <a href="https://github.com/eldo9000/TurboTalk/releases/latest/download/TurboTalk-macOS-arm64.dmg">
    <img src="https://img.shields.io/badge/Download-macOS%20Apple%20Silicon-blue?style=for-the-badge&logo=apple&logoColor=white" alt="Download for macOS (Apple Silicon)" />
  </a>
  &nbsp;
  <a href="https://github.com/eldo9000/TurboTalk/releases/latest/download/TurboTalk-Windows-x64-setup.exe">
    <img src="https://img.shields.io/badge/Download-Windows%20x64-0078d4?style=for-the-badge&logo=windows&logoColor=white" alt="Download for Windows (x64)" />
  </a>
</p>
<p align="center"><sub>1.0 supports macOS Apple Silicon and Windows x64. Linux is the 2.0 track.</sub></p>

---

<table>
  <tr>
    <td align="center">
      <p><img src="docs/assets/ss2.jpg" alt="Models tab" /></p>
      <p><sub>Whisper model selection</sub></p>
    </td>
    <td align="center">
      <p><img src="docs/assets/ss1.jpg" alt="Settings tab" /></p>
      <p><sub>Hotkey, recording, and system settings</sub></p>
    </td>
  </tr>
</table>

---

## What it does

Free, local voice dictation. Your words appear wherever your cursor was — in any app, any text field.

No cloud, no account, no subscription. Everything runs on your machine. The model downloads itself on first launch. You don't need to know which one to pick.

## Why it's straightforward

Most voice tools make you hunt for a model, configure a server, and figure out which settings matter. This one ships with everything included and picks sensible defaults. Install it, walk through a three-step onboarding, and you're dictating.

- **Push-to-talk or tap** — hold a key to record, or tap once to toggle
- **Model downloads itself** — the right one is pre-selected and downloads on first run
- **Three cleanup modes** — paste raw, fix punctuation automatically, or run a local AI that formats prose, code, and shell commands differently
- **Last 50 dictations saved** — click any entry to copy it back
- **Nothing leaves your machine** — not in the settings, not in the codebase

## Models

The app ships with two transcription engines. Parakeet is the default — it's the fastest and downloads automatically.

| Engine | Default model | Size | Best for |
|---|---|---|---|
| **Parakeet** *(default)* | Parakeet TDT 0.6B v2 | ~660 MB | Fast English dictation. Ships as the default. |
| Whisper | large-v3-turbo | 1.6 GB | Best multilingual accuracy. |

You can switch engines and swap models in the Models tab. The app downloads whatever you choose — no manual file hunting.

## Install

1. Download and open the installer for your platform.
2. **macOS:** drag Turbo Talk into `/Applications`. First launch: **right-click → Open** to get past the Gatekeeper warning (expected for unsigned apps).
   **Windows:** run the installer. If SmartScreen appears, choose **More info → Run anyway** (expected for unsigned apps).
3. Walk through the three-step onboarding: grant Accessibility, grant Microphone, download a model.
4. Set your trigger key. Hold it, say something, let go.

**macOS — if the hotkey does nothing after install:** go to System Settings → Privacy & Security → Accessibility, remove Turbo Talk, re-add it, and relaunch. One-time step required by macOS for ad-hoc signed apps.

## Cleanup modes

**Off** — paste exactly what the model heard.

**Simple** — fix capitalization and punctuation automatically. No extra model needed.

**Advanced** — a local [Ollama](https://ollama.com) model reads what you said and formats it differently depending on whether it sounds like prose, code, or a shell command. Requires Ollama installed separately.

## A few other things

- `Cmd+=` / `Cmd+−` zoom the UI. `Cmd+0` resets. Persists across launches.
- Closing the window hides to tray. The hotkey stays active. Quit from the tray menu to fully exit.
- Config and history live at `~/.config/turbotalk/`. Delete that folder to wipe everything.

---

*The 1.0 release is unsigned/ad-hoc on macOS and Windows. Linux builds are deferred until the 2.0 release.*
