# TASK-3: Improve failure-mode error messages for actionable beta failures

## Goal
The five highest-friction beta failure modes — mic permission denied, sidecar missing, model missing, Whisper non-zero exit, and Ollama unreachable — each produce a specific, actionable error string visible in the UI rather than a generic "transcription failed" or empty state.

## Context
TurboTalk is a Tauri 2 + Svelte 5 dictation app on macOS. The Rust backend emits errors through Tauri events or command return values. The frontend displays them in a status/error area.

Current behavior: most failures collapse to a generic error string. Beta users cannot tell whether they need to grant a permission, install a model, or start Ollama.

Target behavior per failure mode:

| Failure | Message to show |
|---|---|
| Mic permission denied (no input devices found) | "Microphone access denied — grant permission in System Settings → Privacy → Microphone, then relaunch." |
| Sidecar binary missing or not executable | "Whisper sidecar not found. Reinstall the app or check that whisper-cli exists in the app bundle." |
| Model file missing | "Whisper model not found at the configured path. Open Settings and set the correct model path." |
| Whisper exits non-zero | "Transcription failed (whisper-cli error <code>). Check that the model file is valid." |
| Ollama unreachable (cleanup mode = advanced) | "Cannot reach Ollama at <url>. Start Ollama or switch to a simpler cleanup mode in Settings." |

Error strings should be returned from the backend as typed error variants, not constructed in the frontend. The frontend just displays whatever string it receives.

Key files:
- `src-tauri/src/transcribe.rs` — sidecar invocation, model path check, exit-code handling
- `src-tauri/src/audio.rs` — mic device availability
- `src-tauri/src/cleanup.rs` — Ollama HTTP call
- `src-tauri/src/lib.rs` — command wiring
- `src/App.svelte` — error display

Check each file for the current error return path before editing. Only change the error message strings — do not restructure error handling, change control flow, or add new error types beyond what is needed to carry the message to the frontend.

## In scope
- `src-tauri/src/transcribe.rs` — sidecar missing, model missing, non-zero exit messages
- `src-tauri/src/audio.rs` — no input devices message
- `src-tauri/src/cleanup.rs` — Ollama unreachable message
- `src/App.svelte` — verify error strings are rendered where the user can see them (no UI redesign needed, just confirm the path exists)

## Out of scope
- Adding new error types or restructuring the error enum (use existing error path, change strings only)
- The diagnostics command or panel (TASK-1, TASK-2)
- SMOKE-TEST.md (TASK-4)
- Privacy, history, PRIVACY.md (TASK-5–7)
- Windows or Linux platform error handling

## Steps
1. Read `transcribe.rs` — find where sidecar path is resolved and where the process exit code is checked. Update the error strings for "not found", "not executable", and "non-zero exit" to match the table above. Include the exit code in the non-zero-exit message.
2. Read `audio.rs` — find where input devices are enumerated. If no devices are found and recording is attempted, ensure the error string matches "Microphone access denied…" above.
3. Read `cleanup.rs` — find the Ollama HTTP call. Update the error string for a failed/timeout connection to match "Cannot reach Ollama at <url>…" above, interpolating the actual URL from settings.
4. Read `src/App.svelte` — confirm that error strings returned from these commands are rendered in the UI. If they are already displayed, no frontend change is needed. If they are silently swallowed, add a one-line error display (no new component).
5. Run `cargo clippy -D warnings` — must exit 0.

## Success signal
`cargo clippy -D warnings` exits 0. In manual testing: renaming the model file and triggering a dictation shows "Whisper model not found…" in the UI. Stopping Ollama and triggering a dictation with cleanup mode = advanced shows "Cannot reach Ollama…" in the UI.
