# TurboTalk

Voice dictation for getting work done.

---

## Setup

- Download a model from the Models tab. 
- Set your trigger key and mode in Settings. 
- Start dictating.

---

## Release matrix

First beta is **macOS on Apple Silicon only.** The hotkey and paste paths are
implemented against macOS APIs (CGEventTap, `osascript`); on every other
target they compile to honest "unsupported platform" stubs. No Windows or
Linux build is shipped.

| Platform | Architecture | First-beta status |
|---|---|---|
| macOS    | arm64 (Apple Silicon) | **supported (beta-1)** |
| macOS    | x86_64 (Intel)        | not supported — no Intel sidecar built |
| Windows  | x86_64                | not supported — deferred (no hotkey/paste implementation) |
| Linux    | x86_64                | not supported — deferred (no hotkey/paste implementation) |

### Install

Download `TurboTalk-<version>-macos-arm64.dmg` from the release page, drag
`TurboTalk.app` to `/Applications`, then **right-click → Open** the first
time you launch it. Beta-1 builds are ad-hoc signed, so Gatekeeper will
warn the first time. If you'd rather build from source, see `BUILD.md`.

### Permissions the app will request

- **Microphone** — to capture audio while you hold the trigger key. Without
  this, no recording happens.
- **Accessibility** (System Settings → Privacy & Security → Accessibility) —
  required twice over: (1) for the global push-to-talk hotkey, which uses a
  `CGEventTap` to observe modifier-key flag changes, and (2) for the paste
  step, which sends `Cmd+V` to the focused app via `System Events`. If you
  see a `paste-error` toast saying "check Accessibility permission", this is
  why.

No other system permissions are requested. There is no Automation prompt
per app, no Full Disk Access, no Screen Recording.

### Local data

- **Config + history:** `~/.config/librewin/turbotalk/` — holds
  `config.toml` (settings) and `history.json` (last 50 dictations).
- **Whisper models:** `~/.config/librewin/turbotalk/models/` — `.bin` files
  downloaded via the Models tab live here.
- **Audio temp files:** `turbotalk-*.wav` written to the system temp dir
  (`/tmp` on macOS) for each dictation. Each file is deleted automatically
  the moment its dictation finishes — successful, failed, or cancelled.
- **Delete everything:** quit from the tray, then
  `rm -rf ~/.config/librewin/turbotalk/`.

### Known limitations

- Apple Silicon only. No Intel-Mac, Windows, or Linux build.
- Ad-hoc signed only — not Apple-notarized. Expect a Gatekeeper warning on
  first launch.
- No auto-updater. Re-download the DMG to update.
- History is saved to disk by default, retained for 10 days. Configurable
  in Settings — choose `restart` (clear on launch), `1d`, `5d`, `10d`, or
  `30d`. Capped at 50 entries either way.
- The Chaperone cleanup mode requires a local Ollama install. If you don't
  run Ollama, leave cleanup on `Off` or `Simple`.

### Feedback

Personal-use beta — feedback by direct message until a public tracker
opens.

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

## For Engineers

The parts that don't show up in a feature list but represent most of the real work, this is actually why Turbo Talk is well made:

- **Explicit audio pipeline contract.** Every recording goes through a fixed, non-negotiable path: native mic capture → downmix mono → resample to 16 kHz → Silero VAD trim → min-duration reject → peak normalize to −1 dBFS → write 16 kHz mono 16-bit PCM WAV. This order is documented, tested, and enforced with named constants. No codec detours. No "supported formats." One correct format for Whisper.
- **Stage timing on every dictation.** Post-release audio finalization is instrumented: downmix, resample, VAD, normalize, and WAV write each emit a timing entry. Optimization decisions are based on measured evidence, not assumptions.
- **Strictly one in-flight dictation job.** The recorder has a full 6-state lifecycle: `Ready → Recording → FinalizingAudio → Transcribing → Cleaning → Pasting → Ready`. Pressing the hotkey while a job is in any non-Ready state is handled explicitly — a `dictation-busy` event fires, no second job spawns. The foundation for a deliberate queue is there when it's needed.
- **Transcription, cleanup, and paste are separate named stages.** Whisper runs and returns raw text. Cleanup runs as its own stage. Paste runs as its own stage. Each has its own lifecycle state transition and its own timing. "Transcribing" means Whisper only — not Whisper-plus-postprocessing silently bundled together.
- **Paste target is observable.** The frontmost app is captured at recording start and again immediately before paste. Both are logged with the job id. If focus changed between the two, a `focus-changed-before-paste` event fires and surfaces a recoverable UI banner. No silent paste into the wrong window.
- **Silero VAD session reuse.** VAD model initialization (ONNX session construction) is not paid on every dictation. The session is held and reused with per-call state isolation to ensure no speech bounds from a prior recording can influence the next.
- **Persistent Whisper worker (in progress).** The current model spawns a fresh `whisper-cli` process per recording. Whisper's dominant cost is model load and Metal context setup — not inference. A persistent transcription worker that keeps the model warm between dictations is the next major latency win.

## License

MIT.
