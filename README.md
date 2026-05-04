# TurboTalk

Voice dictation for getting work done.

---

## Supported platforms

| OS | Arch | Install | Signed? | Known limits |
|---|---|---|---|---|
| macOS 12+ | arm64 (Apple Silicon) | `.dmg` | Ad-hoc only (not notarized) | First launch needs right-click → Open |
| Windows 10 (1809+) / 11 | x64 | `.exe` (NSIS) — **beta in progress** | Unsigned | SmartScreen warns on first run; needs WebView2 runtime |
| Linux | x64 | `.AppImage` — **beta in progress** | Unsigned | **X11 only — Wayland not supported.** Needs FUSE (`libfuse2`) |

Windows and Linux artifacts are not yet published to the releases page. The build procedures are in `RELEASING.md` and will produce real artifacts once the bundled Whisper sidecar binaries for those platforms land. Until then, only the macOS DMG is downloadable.

---

## Install — macOS

1. Download the DMG from the [releases page](https://github.com/eldo9000/TurboTalk-App/releases) and drag `Turbo Talk.app` into `/Applications`.
2. First launch: **right-click → Open** (the beta is ad-hoc signed, not notarized — macOS will refuse a normal double-click the first time).
3. Walk through the three-step onboarding wizard: grant Accessibility → grant Microphone → pick a transcription model.
4. Open Settings, set your trigger key, choose hold-to-talk or toggle.
5. Open TextEdit, hold the trigger key, say "hello world", release — your sentence appears at the cursor.

## Install — Windows

> Beta in progress — once the Win sidecar binary is bundled, the `.exe` will appear on the releases page.

1. Download `TurboTalk-<version>-windows-x64-setup.exe` from the releases page.
2. Double-click. Windows SmartScreen will show **"Windows protected your PC"** because the installer is unsigned. Click **More info → Run anyway** to proceed.
3. Complete the NSIS installer.
4. If the app fails to launch with a WebView2 error (most common on Windows 10), install the WebView2 Evergreen runtime from <https://developer.microsoft.com/microsoft-edge/webview2/>. Windows 11 ships with it preinstalled.
5. Launch TurboTalk from the Start menu, complete onboarding, set your trigger key.
6. Open Notepad, hold the trigger key, say "hello world", release — your sentence appears at the cursor.

## Install — Linux (X11)

> Beta in progress — once the Linux sidecar binary is bundled, the AppImage will appear on the releases page.

1. **Confirm you are on an X11 session.** Run `echo $XDG_SESSION_TYPE` — it must print `x11`. If it prints `wayland`, log out and pick the "Xorg" or "X11" variant of your desktop at the login screen. See the Wayland note below.
2. Install FUSE if your distro doesn't have it: `sudo apt install libfuse2` (Debian/Ubuntu) or your distro's equivalent. AppImage requires FUSE to run.
3. Download `TurboTalk-<version>-linux-x64.AppImage`, mark it executable, and run it:

   ```bash
   chmod +x TurboTalk-<version>-linux-x64.AppImage
   ./TurboTalk-<version>-linux-x64.AppImage
   ```

4. Complete onboarding, set your trigger key.
5. Open `gedit` (or any text editor), hold the trigger key, say "hello world", release — your sentence appears at the cursor.

### Wayland note

Wayland is not supported and is not on the roadmap for this beta. Wayland compositors deliberately block the kind of system-wide keystroke injection TurboTalk needs to paste your transcript into the focused app — that's a security feature of the protocol, not an app bug. On X11, those primitives (`xtest` / `XSendEvent`) work; on Wayland, the compositor refuses them and there is no portable replacement. If your distro defaults to Wayland, log out and choose the X11 / Xorg variant of your desktop at the login screen.

---

## Why this exists

Every other dictation tool is built around a feature list. You get a dropdown of 40 models, a settings panel with 12 tabs, and a history buried three clicks deep. They optimized for "supported" instead of "usable."

---

## Features

### History you can reach in one click

Your last 50 dictations are on the History tab. Click any entry to copy it. That's it. No hunting through a separate panel, no "paste from history" menu buried in a toolbar. The thing you said five minutes ago is one click away.

### A UI you can actually read

Zoom from 100% to 200% in 25% steps with `Cmd+=` and `Cmd+−` (Ctrl on Win/Linux). Resets to 100% with `Cmd+0` / `Ctrl+0`. Zoom level persists. If you're dictating from across the room or you just want bigger text, you get bigger text — without touching a settings page.

### Three models, one obvious choice

Most apps ship a wall of models and leave you to figure out which one to use. TurboTalk ships three:

| Model | Size | When to use |
|---|---|---|
| **large-v3-turbo** *(recommended)* | 1.6 GB | Daily use. Fast, accurate, multilingual. This is the one. |
| large-v3-turbo-q5_0 | 574 MB | Constrained disk space. Some accuracy tradeoff. |
| large-v3 | 3.1 GB | Maximum accuracy. Noticeably slower. |

The recommended model works for 95% of people — tested across office work (emails, docs, meeting notes), coding (variable names, comments, commit messages), and creative work (drafts, brainstorming, longform). Unless you're doing something specialized, you don't need anything else. It's labeled clearly. Install it and move on.

If you already have a Whisper `.bin` file you trust, paste the path. It works.

### Stays out of your way

- **Fully customizable trigger key.** Press-and-hold or toggle. On macOS: left or right Option, Control, Command, or Shift, or any numpad key. On Windows/Linux: equivalent modifiers (Alt, Ctrl, Shift, Win/Super) and numpad keys.
- **Status indicator in the titlebar/tray** while recording and transcribing. You always know the state.
- **Close hides to tray.** The app doesn't quit when you close the window — it's still listening for your hotkey. Quit from the tray when you actually mean it.
- **Error toasts are specific and clickable.** "Recording too short" tells you the duration. A missing-permission toast deep-links you straight to the right system settings pane — no hunting.

### Cleans up your speech (optionally)

Raw Whisper output is good. It can be better. Three levels:

- **Off** — paste exactly what Whisper heard.
- **Simple** — capitalize the first letter, strip filler words and Whisper artifacts. No network, no model, instant.
- **Chaperone** — route through a local Ollama model that understands whether you're dictating prose, code, or a shell command, and formats accordingly. Bring your own vocabulary and prompt.

---

## For Engineers

A simple app is not an excuse for sloppy work. The parts that don't show up in a feature list but represent most of the real effort:

- **Tight code, real security review.** Every IPC boundary, file read, shell-out, and untrusted-input path has been audited for the obvious bug classes. No opaque cloud SDKs in the dependency graph. Local-only by design — no cloud calls, no telemetry, no analytics — and that's enforced at the architecture level, not buried in a privacy policy.
- **Built to not freeze under fire.** Dictation is rapid-fire — you'll mash the hotkey before the last paste finishes, switch focus mid-recording, walk away with AirPods still on. Every one of those paths has an explicit handler that recovers cleanly. You'll see a banner; you won't see a hang. The recorder is a single-job state machine, so a phantom second recording can't race the first one's paste.
- **One deliberate audio chain.** Whatever mic you record from, every recording hits the same fixed pipeline before Whisper sees it: downmix → resample to 16 kHz → voice-activity trim → loudness normalize → clean WAV. Same shape in, same accuracy out. Each stage is timed per dictation, so optimization runs on measured numbers, not vibes.
- **Power users aren't an afterthought.** Chaperone mode routes your transcript through a local Ollama model with your own vocabulary list, your own classifier prompt, and per-context formatting (prose vs. code vs. shell command). Off and Simple modes are there if you don't need it — the headroom is there if you do.

---

### Permissions the app will request

The onboarding wizard walks you through whatever your platform requires. You should never have to find these manually.

**macOS**

- **Microphone** — to capture audio while you hold the trigger key.
- **Accessibility** (System Settings → Privacy & Security → Accessibility) — required twice over: (1) for the global push-to-talk hotkey, which uses a `CGEventTap` to observe modifier-key flag changes, and (2) for the paste step, which sends `Cmd+V` to the focused app via `System Events`. If you see a `paste-error` toast saying "check Accessibility permission", this is why.

No other macOS system permissions are requested. There is no Automation prompt per app, no Full Disk Access, no Screen Recording.

**Windows**

- **Microphone** — Settings → Privacy & security → Microphone. Windows will prompt the first time TurboTalk records.
- No other system permissions are needed. Global hotkey + paste injection do not require any explicit grant on Windows.

**Linux (X11)**

- No system permissions are requested. PulseAudio/PipeWire grants mic access by default, and X11 allows the global hotkey + paste injection without explicit per-app permission.

### Local data

- **Config + history (macOS):** `~/.config/librewin/turbotalk/` — holds `config.toml` (settings) and `history.json` (last 50 dictations).
- **Config + history (Windows):** `%APPDATA%\librewin\turbotalk\` — same files.
- **Config + history (Linux):** `~/.config/librewin/turbotalk/` — same files.
- **Whisper models:** under the same `librewin/turbotalk/models/` directory — `.bin` files downloaded via the Models tab live here.
- **Audio temp files:** `turbotalk-*.wav` written to the system temp dir (`/tmp` on macOS/Linux, `%TEMP%` on Windows) for each dictation. Each file is deleted automatically the moment its dictation finishes — successful, failed, or cancelled.
- **Delete everything:** quit from the tray, then delete the `librewin/turbotalk/` directory listed for your platform above.

### Known limitations

- **macOS:** Apple Silicon only. Ad-hoc signed only — not Apple-notarized. Expect a Gatekeeper warning on first launch (right-click → Open the first time).
- **Windows:** Unsigned `.exe` — SmartScreen will show a "Windows protected your PC" warning the first time you run the installer. Click **More info → Run anyway** to proceed. WebView2 runtime is required (preinstalled on Windows 11; Windows 10 users may need <https://developer.microsoft.com/microsoft-edge/webview2/>).
- **Linux:** X11 only — **Wayland is not supported.** AppImage requires FUSE (`libfuse2` on Debian/Ubuntu). Tray-icon support depends on your desktop's AppIndicator support (GNOME may need an extension).
- **All platforms:** No auto-updater. Re-download to update.
- History is saved to disk by default, retained for 10 days. Configurable in Settings — choose `restart` (clear on launch), `1d`, `5d`, `10d`, or `30d`. Capped at 50 entries either way.
- The Chaperone cleanup mode requires a local Ollama install. If you don't run Ollama, leave cleanup on `Off` or `Simple`.

### Feedback

Personal-use beta — feedback by direct message until a public tracker opens.
