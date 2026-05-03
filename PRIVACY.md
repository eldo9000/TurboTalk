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

Transcripts are not stored permanently unless you have enabled history in Settings.

- **History disabled (default):** The transcript is injected into the focused app and discarded. Nothing is written to disk.
- **History enabled:** Each transcript is appended as a JSON record under `~/.config/librewin/turbotalk/history/`. Files in that directory are plain JSON; you can read, edit, or delete them at any time.

---

## Network calls

Two situations trigger a network call:

1. **Model download (user-initiated):** When you download a Whisper model from within the app, TurboTalk fetches it from the configured model source. This is only triggered by your explicit action in the UI.

2. **Advanced cleanup (localhost only):** If you enable Advanced cleanup mode, TurboTalk sends the transcript text string to `http://localhost:11434` — the default address for a locally running Ollama server. This is a request to your own machine, not a remote server. No data leaves your computer.

No other network calls are made. There is no telemetry, no analytics, no crash reporting, and no update-check traffic.

---

## What Advanced cleanup sends to Ollama

When Advanced cleanup is enabled, TurboTalk sends the transcript text string to your local Ollama instance. It does not send audio, microphone metadata, timestamps, app context, or any other information. The request goes to `http://localhost:11434` and stays on your machine.

---

## Logs

Application logs do not contain the body of any transcript. Transcript content is reduced to a character count before logging (e.g., `"transcript: 142 chars"`). Log files, if written, are stored in the standard macOS application log location and do not leave your machine.

---

## How to delete everything

To remove all data TurboTalk stores on your machine:

| What | Path |
|------|------|
| Settings | `~/.config/librewin/turbotalk/settings.json` |
| History | `~/.config/librewin/turbotalk/history/` (entire directory) |
| Model files | `~/.config/librewin/turbotalk/models/` (entire directory, or whichever path you configured) |
| App bundle | Move TurboTalk.app from `/Applications` to Trash |
| Launch Agent (if autostart was enabled) | `~/Library/LaunchAgents/com.librewin.turbotalk.plist` |

After removing the Launch Agent plist, run `launchctl unload ~/Library/LaunchAgents/com.librewin.turbotalk.plist` (before deleting it) to stop the agent from being loaded in the current session.

Temp audio files are deleted automatically after each recording. If a recording was interrupted abnormally, any leftover `.wav` file in your system temp directory can be deleted safely.

---

## Questions or concerns

Open an issue at the project repo.
