# TurboTalk

Voice dictation for getting work done.

---

## Setup

1. Download the DMG from the [releases page](https://github.com/eldo9000/TurboTalk-App/releases) and drag `Turbo Talk.app` into `/Applications`.
2. First launch — right-click → **Open** (the beta is ad-hoc signed, not yet Apple-notarized; macOS will refuse a normal double-click the first time).
3. Walk through the three-step onboarding wizard: grant Accessibility → grant Microphone → pick a transcription model (the recommended one is the right answer for almost everyone).
4. Open Settings, set your trigger key — left or right Option / Control / Command / Shift, or any numpad key — and choose hold-to-talk or toggle.
5. Start dictating.

---

## Why this exists

Every other dictation tool is built around a feature list. You get a dropdown of 40 models, a settings panel with 12 tabs, and a history buried three clicks deep. They optimized for "supported" instead of "usable."

---

## Features

### History you can reach in one click

Your last 50 dictations are on the History tab. Click any entry to copy it. That's it. No hunting through a separate panel, no "paste from history" menu buried in a toolbar. The thing you said five minutes ago is one click away.

### A UI you can actually read

Zoom from 100% to 200% in 25% steps with `Cmd+=` and `Cmd+−`. Resets to 100% with `Cmd+0`. Zoom level persists. If you're dictating from across the room or you just want bigger text, you get bigger text — without touching a settings page.

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

- **Fully customizable trigger key.** Press-and-hold or toggle. Left or right Option, Control, Command, or Shift, or any numpad key. The overlay shows a waveform while you speak and "Transcribing…" while it works. Then it disappears.
- **Red dot in the titlebar** while recording. Amber while transcribing. Gone when done. You always know the state.
- **Close hides to tray.** The app doesn't quit when you close the window — it's still listening for your hotkey. Quit from the tray when you actually mean it.
- **Error toasts are specific and clickable.** "Recording too short" tells you the duration. A missing-permission toast deep-links you straight to the right pane in System Settings — no hunting.

### Cleans up your speech (optionally)

Raw Whisper output is good. It can be better. Three levels:

- **Off** — paste exactly what Whisper heard.
- **Simple** — capitalize the first letter, strip filler words and Whisper artifacts. No network, no model, instant.
- **Chaperone** — route through a local Ollama model that understands whether you're dictating prose, code, or a shell command, and formats accordingly. Bring your own vocabulary and prompt.

---

## For Engineers

The parts that don't show up in a feature list but represent most of the real work — and why this isn't a vibe-coded weekend project:

- **First-launch can't fail silently.** A three-step readiness gate (Accessibility, Microphone, a working transcription model) sits in front of the main UI. The hotkey doesn't get bound, the recorder doesn't get spawned, until all three are green. If you revoke a permission later or delete the model file, the wizard re-arms the next time you click back into the window — no dead push-to-talk that takes ten minutes to diagnose.
- **One audio path, no codec gymnastics.** Whatever mic you record from — built-in, AirPods, USB interface — every recording hits the same fixed six-stage pipeline before Whisper sees it. Same sample rate, same loudness target, same trim rules. A bug in one mic isn't a bug in all of them.
- **Every dictation logs its own timing.** Each stage (downmix, resample, voice-activity trim, normalize, write, transcribe, paste) is timed and stamped with a per-job id. Performance work runs against measured numbers, not vibes.
- **You can't start two dictations at once.** The recorder is a six-state lifecycle. Mash the hotkey while one's in flight and you get a "busy" event — never a phantom second job racing the first one's paste.
- **The paste destination is verified, not assumed.** The frontmost app is captured when recording starts and again immediately before paste. If they're different, you get a banner — the transcript still goes through, but you know the focus changed and can react. No silent paste into the wrong window.
- **Hot models stay hot.** The voice-activity-detection session is built once and reused across every recording, so each dictation doesn't pay a cold-start tax for an ONNX session it just tore down.
- **Local-only by design.** All transcription, cleanup, history — your machine, every time. No cloud calls, no telemetry, no analytics. The optional Chaperone cleanup uses a local Ollama install; everything else is fully offline.
- **Manual updates are a deliberate choice.** Auto-update is a code-execution channel into your machine. Until there's a long-lived signing key with a documented rotation/loss procedure and stable artifact hosting, shipping new versions = re-download the DMG. Slower, but a known quantity.

---

### Permissions the app will request

The onboarding wizard walks you through both. You should never have to find these manually.

- **Microphone** — to capture audio while you hold the trigger key. Without this, no recording happens.
- **Accessibility** (System Settings → Privacy & Security → Accessibility) — required twice over: (1) for the global push-to-talk hotkey, which uses a `CGEventTap` to observe modifier-key flag changes, and (2) for the paste step, which sends `Cmd+V` to the focused app via `System Events`. If you see a `paste-error` toast saying "check Accessibility permission", this is why.

No other system permissions are requested. There is no Automation prompt per app, no Full Disk Access, no Screen Recording.

### Local data

- **Config + history:** `~/.config/librewin/turbotalk/` — holds `config.toml` (settings) and `history.json` (last 50 dictations).
- **Whisper models:** `~/.config/librewin/turbotalk/models/` — `.bin` files downloaded via the Models tab live here.
- **Audio temp files:** `turbotalk-*.wav` written to the system temp dir (`/tmp` on macOS) for each dictation. Each file is deleted automatically the moment its dictation finishes — successful, failed, or cancelled.
- **Delete everything:** quit from the tray, then `rm -rf ~/.config/librewin/turbotalk/`.

### Known limitations

- Apple Silicon only. No Intel-Mac, Windows, or Linux build.
- Ad-hoc signed only — not Apple-notarized. Expect a Gatekeeper warning on first launch (right-click → Open the first time).
- No auto-updater. Re-download the DMG to update.
- History is saved to disk by default, retained for 10 days. Configurable in Settings — choose `restart` (clear on launch), `1d`, `5d`, `10d`, or `30d`. Capped at 50 entries either way.
- The Chaperone cleanup mode requires a local Ollama install. If you don't run Ollama, leave cleanup on `Off` or `Simple`.

### Feedback

Personal-use beta — feedback by direct message until a public tracker opens.
