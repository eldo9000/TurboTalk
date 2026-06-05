<p align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" alt="Turbo Talk" />
</p>

<h1 align="center">Turbo Talk</h1>

<p align="center"><strong>Go fast.</strong></p>
<p align="center"><strong>Free voice dictation for getting work done.</strong></p>

<p align="center">
  <a href="https://github.com/eldo9000/TurboTalk-App/releases/latest/download/TurboTalk-macOS-arm64.dmg">
    <img src="https://img.shields.io/badge/Download-macOS%20Apple%20Silicon-blue?style=for-the-badge&logo=apple&logoColor=white" alt="Download for macOS (Apple Silicon)" />
  </a>
  &nbsp;
  <a href="https://github.com/eldo9000/TurboTalk-App/releases/latest/download/TurboTalk-0.9.5-windows-x64-setup.exe">
    <img src="https://img.shields.io/badge/Download-Windows%20x64%20Beta-0078d4?style=for-the-badge&logo=windows&logoColor=white" alt="Download for Windows (x64) — Beta" />
  </a>
</p>
<p align="center"><sub>Windows build is a beta — hotkey and paste work on real hardware; end-to-end testing is ongoing.</sub></p>

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

Hold a key. Talk. Let go. Your words appear wherever your cursor was.

No cloud, no subscription, no account. Everything runs on your Mac. I've been using it every day for weeks and haven't needed to touch the settings once.

## Why it's like this

Most voice tools give you fifteen model options and a settings panel that takes an afternoon to figure out. This one gives you three models — the ones that actually work well — and gets out of the way.

- **Hold or tap** a trigger key of your choice to record
- **Three Whisper models** to download — picked because they're all good, not just to fill a list
- **Three cleanup modes** — paste raw, clean up punctuation, or run through a local AI model that formats prose, code, and shell commands differently
- **Last 50 dictations saved** — click any entry to copy it back
- **Nothing leaves your machine.** Not in the settings, not in the codebase.

The app is basically done. It'll get bug fixes. That's a feature, not a limitation.

## Models

| Model | Size | Notes |
|---|---|---|
| **large-v3-turbo** *(start here)* | 1.6 GB | Fast and accurate. Most people stop here. |
| large-v3-turbo-q5_0 | 574 MB | Smaller download, slightly less accurate. Good if storage is tight. |
| large-v3 | 3.1 GB | Highest accuracy. Worth it if you dictate constantly. |

Three choices because those are the three worth having.

## Install

1. Download the DMG and drag **Turbo Talk** into `/Applications`.
2. First launch: **right-click → Open.** macOS will warn you it's unsigned — that's expected. Right-click gets past it.
3. Walk through the three-step onboarding: grant Accessibility, grant Microphone, download a model.
4. Set your trigger key. Hold it, say something, let go. Done.

**If the hotkey does nothing after install:** go to System Settings → Privacy & Security → Accessibility, remove Turbo Talk, re-add it, toggle it on, relaunch. One-time step — macOS requires it the first time for ad-hoc signed apps.

## Cleanup modes

**Off** — paste exactly what Whisper heard.

**Simple** — clean up capitalization and obvious filler words. No model needed, works instantly.

**Advanced** — a local [Ollama](https://ollama.com) model reads what you said and formats it: prose, code, or shell commands. The vocabulary list and classifier prompt are both editable if you want to tune it. Requires Ollama installed separately.

## A few other things

- `Cmd+=` / `Cmd+−` zoom the UI in 25% steps. `Cmd+0` resets. Persists across launches.
- Closing the window hides to tray. The hotkey stays active. Quit from the tray menu to fully exit.
- History retention is configurable: restart, 1d, 5d, 10d, or 30d. Capped at 50 entries.
- Config and history live at `~/.config/librewin/turbotalk/`. Delete that folder to wipe everything.

---

*Linux builds exist but are not yet ready for use. Windows is in beta — the full dictation loop works on real hardware; if you run into issues, open a GitHub issue.*
