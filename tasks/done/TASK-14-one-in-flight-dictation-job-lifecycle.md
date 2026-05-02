# TASK-14: Make the dictation job lifecycle explicit and allow only one in-flight job

## Goal
TurboTalk has one authoritative backend lifecycle for a dictation job, with no overlapping recording/transcription/paste work. Pressing the hotkey while the app is busy must be handled explicitly instead of accidentally starting a second job.

Target lifecycle:

`Ready → Recording → FinalizingAudio → Transcribing → Cleaning → Pasting → Ready`

## Context
Today `Recorder` has three states: `Ready`, `Recording`, `Transcribing`. `recorder.stop()` transitions back to `Ready` immediately after the WAV is produced, while `hotkey.rs` continues Whisper, cleanup, and paste on a spawned worker thread. That means a new recording can begin while a previous transcription/paste is still in flight.

For the first stable version, the safer rule is one in-flight job total. Later, if rapid-fire dictation is desired, a queue can be added deliberately with job ids and paste-target rules.

## In scope
- `src-tauri/src/recorder.rs`
  - Expand the state enum to include `FinalizingAudio`, `Transcribing`, `Cleaning`, and `Pasting`.
  - Add explicit transition helpers or methods that make illegal transitions visible.
  - Keep typed `RecorderError::IllegalTransition`.
- `src-tauri/src/hotkey.rs`
  - Do not start recording unless state is `Ready`.
  - Emit a lightweight `dictation-busy` event when the user presses the hotkey while non-ready.
  - Drive stage transitions around audio finalization, transcription, cleanup, and paste.
- Frontend event handling only as needed to avoid confusing UI state.

## Out of scope
- Persistent Whisper.
- VAD session reuse.
- Job queueing.
- Paste-target capture.
- Major frontend visual redesign.

## Steps
1. Read `src-tauri/src/recorder.rs` and `src-tauri/src/hotkey.rs` end-to-end.
2. Add states:
   - `Ready`
   - `Recording`
   - `FinalizingAudio`
   - `Transcribing`
   - `Cleaning`
   - `Pasting`
3. Decide whether `Recorder::stop()` should include only audio finalization or whether `hotkey.rs` should call explicit `begin_transcribing`, `begin_cleaning`, `begin_pasting`, and `finish` methods. Prefer the smallest clear change.
4. Ensure `Recorder` returns to `Ready` on every success, discard, and error path.
5. In `ptt_down`, if `start()` fails because the recorder is busy, emit `dictation-busy` with the current state string. Do not emit `ptt-down`.
6. In `ptt_up`, move through:
   - `FinalizingAudio` while stopping capture and preparing the WAV.
   - `Transcribing` while Whisper runs.
   - `Cleaning` while cleanup/chaperone runs.
   - `Pasting` while paste injection runs.
   - `Ready` after success or failure.
7. Keep the tray icon behavior simple:
   - Recording icon during `Recording`.
   - Transcribing/busy icon for `FinalizingAudio`, `Transcribing`, `Cleaning`, and `Pasting`.
   - Idle icon only after returning to `Ready`.
8. Update `src/App.svelte` and `src/Overlay.svelte` only if required to prevent stale recording/transcribing UI.
9. Add focused unit tests for legal and illegal state transitions in `recorder.rs`.
10. Run:
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
11. Manual test:
   - Start a normal dictation and confirm it still pastes.
   - Press the hotkey again while transcription is running. Confirm no second recording starts and the UI does not flicker into recording.
   - Trigger a too-short recording. Confirm state returns to `Ready`.
   - Trigger a transcription error if practical. Confirm state returns to `Ready`.

## Success signal
- One in-flight job is enforced by the backend.
- Busy hotkey presses are observable and harmless.
- No path leaves the recorder stuck outside `Ready`.
- Normal dictation still works end-to-end.

## Notes
- Do not add a queue in this task. A queue needs separate design for paste order and focused app targeting.
- Keep the state machine easy to read. Typestate is not necessary for this app.

