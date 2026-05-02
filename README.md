# TurboTalk

Voice dictation for getting work done.

---

## Why this exists

Every other dictation tool is built around a feature list. You get a dropdown of 40 models, a settings panel with 12 tabs, and a history buried three clicks deep. They optimized for "supported" instead of "usable."

TurboTalk is built the other way. Every decision starts with: *does this get in the way?* If yes, cut it.

---

## The things that actually matter

### History you can reach in one click

Your last 50 dictations are on the History tab. Click any entry to copy it. That's it. No hunting through a separate panel, no "paste from history" menu buried in a toolbar. The thing you said five minutes ago is one click away.

### A UI you can actually read

Zoom from 100% to 180% with `Cmd+` and `Cmd+−`. Resets to 100% with `Cmd+0`. Zoom level persists. If you're dictating from across the room or you just want bigger text, you get bigger text — without touching a settings page.

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

- **Fully customizable trigger key.** Press-and-hold, toggle, or whatever combination works for how you dictate. Choose from Right Option, Control, Command, or Shift. The overlay shows a waveform while you speak and "Transcribing…" while it works. Then it disappears.
- **Red dot in the titlebar** while recording. Amber while transcribing. Gone when done. You always know the state.
- **Close hides to tray.** The app doesn't quit when you close the window — it's still listening for your hotkey. Quit from the tray when you actually mean it.
- **Error toasts are specific.** "Recording too short" tells you the duration. "Paste failed" tells you to check Accessibility permissions. No generic "something went wrong."

### Cleans up your speech (optionally)

Raw Whisper output is good. It can be better. Three levels:

- **Off** — paste exactly what Whisper heard.
- **Simple** — capitalize the first letter, strip filler words and Whisper artifacts. No network, no model, instant.
- **Chaperone** — route through a local Ollama model that understands whether you're dictating prose, code, or a shell command, and formats accordingly. Bring your own vocabulary and prompt.

---

## No cloud. No account. No telemetry.

Everything runs on your machine. Metal-accelerated on Apple Silicon. Nothing leaves your network.

---

## Setup

Download a model from the Models tab. Set your trigger key and mode in Settings. Start dictating.

---

## License

MIT.
