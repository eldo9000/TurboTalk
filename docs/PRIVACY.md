# TurboTalk Privacy

TurboTalk is a local-first voice dictation tool. In its default configuration, no audio, transcript text, or personal data is sent to any external server. All processing — speech capture, transcription, and optional text cleanup — happens on your machine. The only network activity that can occur is a model download you initiate, or the optional Advanced cleanup feature that contacts a locally running Ollama server (not a remote service).

---

## What is recorded

TurboTalk records audio **only while you are holding the push-to-talk hotkey.** Nothing is captured at rest. There is no background listening, no wake-word detection, and no ambient audio processing.

---

## Audio temp files

When you release the hotkey, the captured audio is written to a temporary WAV file in your system's temp directory (the path returned by `std::env::temp_dir()` — typically `/var/folders/…` on macOS). The filename is randomized for each recording.

That temp file is passed to the `whisper-cli` sidecar for transcription and then **deleted immediately** after transcription completes. The file is not retained, copied elsewhere, or transmitted.

---

## Transcripts and history

Transcript history is saved by default so the History tab survives relaunches.
You can turn this off in Settings with the **Save history** toggle.

- **History enabled (default):** Transcript history is stored in
  `~/.config/librewin/turbotalk/history.json`. It is a plain JSON array capped
  at the newest 50 entries. On macOS and Linux, the history file is created
  with **owner-only permissions** (`0o600`) — other local users cannot read it.
- **History disabled:** New transcripts are injected into the focused app and
  not written to `history.json`. Existing history on disk is left untouched
  until you clear it or delete the file.

---

## Network calls

Two situations trigger a network call:

1. **Model download (user-initiated):** When you download a Whisper model from within the app, TurboTalk fetches it from the configured model source. This is only triggered by your explicit action in the UI.

2. **Advanced cleanup (loopback only):** If you enable Advanced cleanup mode, TurboTalk sends the transcript text string to your configured Ollama URL. The default is `http://localhost:11434`, and the app rejects non-loopback hosts. Only `localhost`, `127.0.0.1`, and `::1` are accepted.

No other network calls are made. There is no telemetry, no analytics, no crash reporting, and no update-check traffic.

---

## What Advanced cleanup sends to Ollama

When Advanced cleanup is enabled, TurboTalk sends the transcript text string to your local Ollama instance. It does not send audio, microphone metadata, timestamps, app context, or any other information. The request goes to the configured loopback Ollama URL and stays on your machine.

---

## Local file permissions

On macOS and Linux (Unix), all directories and files TurboTalk creates under
`~/.config/librewin/turbotalk/` use restricted permissions:
- **Directories** (`config.toml` parent, `logs/`, `models/`) are created with
  **owner-only** permissions (`0o700`) — other local users cannot list or
  traverse them.
- **Files** (`config.toml`, `history.json`, diagnostic reports, log files) are
  created with **owner-only** permissions (`0o600`) — other local users cannot
  read them.

This means that on a shared or multi-user machine, your dictation history,
settings, and diagnostic files are not readable by other local users through
normal filesystem access.

On Windows, TurboTalk uses the standard `%APPDATA%` location, which is already
user-profile-scoped by the operating system.

---

## Logs

Application logs do not contain the body of any transcript. Transcript content is reduced to a character count before logging (e.g., `"transcript: 142 chars"`). Log files, if written, are stored in the standard macOS application log location and do not leave your machine.

---

## Bug reports

When you submit a bug report from the Diagnostics tab, TurboTalk saves a
diagnostic report file locally to `~/.config/librewin/turbotalk/logs/`. This
report includes sanitized configuration, UI events, and recent session logs
— no transcript text, no audio, no personally identifying content.

**By default, bug reports are local-only.** The app never attempts to upload
them unless it was built with the `dev-telegram-bugreport` Cargo feature
enabled, which is intentionally excluded from public release builds. Even in
dev builds with the feature enabled, upload only happens when
`TURBOTALK_BUGREPORT_TG_TOKEN` and `TURBOTALK_BUGREPORT_TG_CHAT` environment
variables are set at build time.

The report file stays on your machine regardless of upload success or failure.

## How to delete everything

To remove all data TurboTalk stores on your machine:

| What | Path |
|------|------|
| Settings | `~/.config/librewin/turbotalk/config.toml` |
| History | `~/.config/librewin/turbotalk/history.json` |
| Model files | `~/.config/librewin/turbotalk/models/` (entire directory, or whichever path you configured) |
| App bundle | Move TurboTalk.app from `/Applications` to Trash |
| Launch Agent (if autostart was enabled) | `~/Library/LaunchAgents/io.librewin.turbotalk.plist` |

After removing the Launch Agent plist, run `launchctl unload ~/Library/LaunchAgents/io.librewin.turbotalk.plist` (before deleting it) to stop the agent from being loaded in the current session.

Temp audio files are deleted automatically after each recording. If a recording was interrupted abnormally, any leftover `.wav` file in your system temp directory can be deleted safely.

---

## Questions or concerns

Open an issue at the project repo.
