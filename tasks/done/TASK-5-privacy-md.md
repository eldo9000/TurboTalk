# TASK-5: Write PRIVACY.md

## Goal
`PRIVACY.md` exists at the repo root. A beta user can read it and answer all five of the following questions without asking the developer: (1) Does my voice leave my machine? (2) Does my transcript leave my machine? (3) Where is history stored? (4) How do I delete history/config/models? (5) What changes if I enable Advanced cleanup?

## Context
TurboTalk is a macOS dictation app. It transcribes locally using whisper-cli sidecar and optionally routes the transcript through a local Ollama server for LLM cleanup ("Advanced" mode). No audio or transcript data is sent to external servers in the default configuration.

Known facts about data handling:
- Audio is captured from the mic, written to a temporary WAV file, passed to whisper-cli, then deleted.
- Temporary WAV file location: system temp directory (`std::env::temp_dir()`), filename randomized per recording.
- Transcripts are not stored permanently unless history is enabled.
- History (if enabled) is stored as JSON under `~/.config/librewin/turbotalk/history/`.
- Settings are stored under `~/.config/librewin/turbotalk/settings.json`.
- Model files are stored wherever the user configured (default: `~/.config/librewin/turbotalk/models/`).
- Network calls only happen if: (a) user downloads a model via the app, or (b) Advanced cleanup mode is enabled (sends transcript text to `http://localhost:11434` — Ollama running locally, not a remote server).
- Logs do not contain transcript body text (redacted to character counts).
- No telemetry, analytics, or crash reporting is sent anywhere.

This is a documentation-only task. Do not change any source code.

The file should be honest and specific. Avoid marketing language. If something is unknown or configurable, say so.

## In scope
- `PRIVACY.md` — new file at repo root

## Out of scope
- Any source code changes
- README changes (README already has a release matrix section)
- History settings changes (TASK-6)
- UI label changes (TASK-7)

## Steps
1. Create `PRIVACY.md` at the repo root.
2. Open with a one-paragraph plain-English summary: local-first, no external servers in default config.
3. Add sections covering:
   - **What is recorded** — mic audio captured during push-to-talk only; nothing recorded at rest
   - **Audio temp files** — where they are written, when they are deleted (immediately after transcription)
   - **Transcripts and history** — where stored, retention setting, how to clear
   - **Network calls** — model download (user-initiated only), Advanced cleanup (localhost Ollama only, not a remote server), nothing else
   - **What Advanced cleanup sends to Ollama** — the transcript text string; no audio, no metadata
   - **Logs** — no transcript body; character counts only
   - **How to delete everything** — exact paths: settings.json, history/ dir, models/ dir, app bundle, LaunchAgent if autostart was enabled
4. Add a final line: "Questions or concerns: open an issue at the project repo."

## Success signal
`PRIVACY.md` exists at the repo root. Reading it, a non-developer beta tester can answer all five questions listed in the Goal without referring to the source code.
