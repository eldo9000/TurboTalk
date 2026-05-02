# TASK-6: Audio module hardening — secure temp files, device-loss detection, clear discard signals

## Goal
The audio module:
- Creates temp WAV files via the `tempfile` crate so they have unpredictable names, owner-only (0600) permissions, and are removed on Drop even if subsequent code panics or returns Err.
- Detects when the active capture device disappears mid-recording (e.g., AirPods disconnect) and explicitly transitions out of Recording, emitting a `device-lost` event the frontend can show.
- Emits a `recording-too-short` event (with the rejected duration in milliseconds) when silence-trimmed output falls under the minimum-length threshold, so users understand why nothing happened.

## Context
TurboTalk is a personal-use macOS voice dictation app. The audio module (`src-tauri/src/audio.rs`) captures from the configured microphone and writes a WAV file to `std::env::temp_dir()` using a name like `turbotalk_<timestamp>.wav`. After transcription succeeds, the WAV is deleted (`remove_file` near the end of `transcribe.rs::run`). Before that delete, multiple things can fail.

A multi-agent review found four converging weaknesses on this module:

1. **Predictable temp filenames** (`audio.rs:199`) — timestamps in milliseconds are guessable to a local attacker. The file lives in `/tmp` with default permissions (often world-readable depending on umask). Any local user can read recently-recorded audio.
2. **No cleanup on error path** (`audio.rs:199` / `transcribe.rs:~66`) — if transcription fails, the WAV is not deleted. It accumulates in `/tmp` indefinitely.
3. **No device-loss detection** (`audio.rs:149`) — when AirPods disconnect mid-recording, the cpal callback continues firing (with zero or stale samples on macOS), the recorder stays in Recording state, and stop() returns either an empty/short WAV (silence-trimmed away) or a corrupt one. The user sees no indication that their device went away — they see "transcription failed" or "nothing happened".
4. **Silent short-recording drop** (`audio.rs:180`) — if the user holds the hotkey for less than ~100ms or speaks below the silence threshold, the WAV is discarded (`stop()` returns `Ok(None)`). No event is emitted. The frontend has no idea — combined with the hotkey-state issues from TASK-5, the overlay can hang.

This task assumes TASK-5 has landed, which adds the `recording-discarded` event to the hotkey path. This task adds a more specific `recording-too-short` event with duration data, plus the new `device-lost` event.

cpal exposes a per-stream error callback via `build_input_stream`'s 4th argument (`error_callback`). On macOS, when a CoreAudio device disappears, this callback fires with a `StreamError::DeviceNotAvailable`. We can use that to flip an atomic flag.

## In scope
- `src-tauri/src/audio.rs` — temp file creation, device-loss detection, threshold metrics
- `src-tauri/src/recorder.rs` — only as needed to expose a "device lost" signal back to the hotkey thread
- `src-tauri/src/hotkey.rs` — emit the new events; transition out of Recording on device loss
- `src-tauri/Cargo.toml` — add `tempfile = "3"` if not already present
- `src/App.svelte` — listen for `device-lost` and `recording-too-short`; show appropriate UI feedback
- `src/Overlay.svelte` — listen for `device-lost` so the overlay clears

## Out of scope
- Any change to the cleanup module (TASK-4) or the recorder state machine semantics (TASK-5)
- Replacing cpal with a different audio library
- Reconnect-on-device-return logic (just stop cleanly; user can press hotkey again)
- Changing the WAV file format or sample rate
- The `unsafe impl Send/Sync` audit (TASK-3) — this task assumes TASK-3 has already established a sound model

## Steps
1. Add `tempfile = "3"` to `[dependencies]` in `src-tauri/Cargo.toml` if not already present. Run `cargo build` to confirm it resolves.
2. Replace the current temp-file creation in `audio.rs` (around line 199) with `tempfile::Builder::new().prefix("turbotalk-").suffix(".wav").tempfile()` to get a `NamedTempFile` with a random name and 0600 perms. Persist the path with `.into_temp_path()` so the file isn't deleted on its Drop until you explicitly call `.persist()` or let the path Drop. Decide on the lifecycle:
   - The cleanest approach: hold a `TempPath` (from `into_temp_path()`) inside the `Recorder`/Audio state across the gap between `stop()` and `transcribe::run()`. When transcription completes (success or error), the `TempPath` is dropped and the file is removed automatically.
   - Alternative: keep the existing String/PathBuf flow but explicitly delete the file in a `Drop` impl on a small RAII guard. Either works — pick the simpler one given the current control flow.
3. Adjust `transcribe.rs` so it accepts whatever path type you chose and so the `remove_file` call is no longer needed (the RAII guard handles it).
4. **Device-loss detection:** in the `cpal::build_input_stream` call inside `audio.rs::open_stream`, supply an `error_callback` (the 4th argument). Inside it, if the error matches `StreamError::DeviceNotAvailable`, set an atomic flag `device_lost: AtomicBool` (add to the AudioCapture struct) to true and clear `is_recording`. Log at warn level.
5. Expose `pub fn device_lost(&self) -> bool` on `AudioCapture` and on `Recorder`. Atomically swap to `false` when read so it's an edge-triggered signal.
6. The level-broadcast thread in `lib.rs` already polls every 50ms while `is_recording` is true. Modify it (or add a parallel poll) to also check `recorder.device_lost()` once per tick — if true, emit `device-lost` to the frontend, set tray to Idle, and force a state transition: call into `recorder.cancel()` (add this method — it returns the recorder to Ready without producing a WAV; cleans up `samples`).
7. **Short-recording event:** in `audio.rs::stop()` where `trimmed.len() < min_samples` returns `Ok(None)`, also stash the trimmed duration in milliseconds on the AudioCapture (e.g., `last_discard_ms: AtomicU32`). Have `Recorder::stop()` return a tagged result: still `Result<Option<PathBuf>>` but if Ok(None), the caller can read `recorder.last_discard_ms()` to get the rejected duration. Alternatively: change the return type to `Result<Either<PathBuf, DiscardReason>>` where DiscardReason carries the ms — pick whichever requires less invasive change.
8. In `hotkey.rs::ptt_up`, when stop returns Ok(None), emit `recording-too-short` with the duration ms as payload (instead of, or in addition to, TASK-5's `recording-discarded`). Decide: `recording-discarded` becomes the catch-all "we threw it away"; `recording-too-short` is the specific subtype with the duration. Frontend can listen to both and prefer the more specific one.
9. **Frontend:** in `src/App.svelte`, add listeners for `device-lost` (show a banner like "Microphone disconnected — pick a different device or reconnect") and `recording-too-short` (subtle non-error toast like "Too short — try holding longer"). In `src/Overlay.svelte`, ensure both events also clear the overlay to idle.
10. Run `cargo build --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`.
11. Manually test:
    - **Temp file lifecycle:** record + transcribe normally. Check `/tmp` — verify no `turbotalk-*.wav` files remain after success. Now force a transcription error (point whisper.bin to a non-existent path or kill whisper-cli mid-run). Verify the WAV file is still cleaned up after the error.
    - **Temp file permissions:** during a recording, run `ls -l /tmp | grep turbotalk` and verify perms are `-rw-------` (0600).
    - **Device loss:** start recording with AirPods, then turn them off (or unplug a USB mic). Verify the overlay clears within ~100-200ms, the tray returns to Idle, and a "device lost" banner appears in the main window.
    - **Short recording:** tap the hotkey for under 100ms (just a flick). Verify the overlay clears immediately and a "too short" indicator appears (or no error, but no hang either).
    - **Normal flow:** PTT + speak + release should still produce transcripts identically to before the change.

## Success signal
- `cargo build` and `cargo clippy -D warnings` exit 0.
- After 5 normal record-transcribe cycles, `ls /tmp/turbotalk-*` returns no files.
- After a forced transcription error, `ls /tmp/turbotalk-*` returns no files.
- `ls -l /tmp` during a recording shows the temp WAV with 0600 permissions.
- Disconnecting the active mic during recording clears the overlay and shows a "device lost" UI signal within ~250ms.
- Holding the hotkey too briefly produces a "too short" UI signal instead of a hang.
- All listeners are present: `grep -n "device-lost\|recording-too-short" src/` returns matches in App.svelte and Overlay.svelte.

## Notes
- `tempfile::NamedTempFile` and `tempfile::TempPath` both implement Drop-based cleanup. Pick whichever fits the existing path-as-String flow best.
- On macOS, the cpal error callback runs on the audio thread. Setting an atomic from there is safe; do not call into Tauri's `app.emit` from the callback (that needs the AppHandle, which you'd have to clone in). Surface the signal to the level-broadcast thread (which already has the AppHandle) instead.
- `recorder.cancel()` should drop the active stream, clear samples, reset `is_recording`, and return state to `Ready` without producing a WAV. The TASK-5 typed state machine should accept this transition.
- If TASK-5 hasn't landed yet, this task can still proceed — adapt the new events to whatever event-emission shape exists.
- Multi-agent review reference: findings SEC-006, SEC-009, ARCH-005, ARCH-008 / MAC-2 in `/tmp/code-analysis-concern-based-main-20260501.md`.
