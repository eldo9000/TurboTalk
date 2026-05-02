# TASK-5: Type-enforce the recorder state machine and stop swallowing hotkey errors

## Goal
The Recorder's state transitions are enforced such that calling `start()` outside `Ready` returns a typed error and calling `stop()` outside `Recording` returns a typed error. The hotkey thread (`hotkey.rs`) does NOT emit `ptt-down`, `ptt-up`, or `transcript` events when the underlying recorder call returned an error. Distinct events `paste-error` and `recording-discarded` exist and are emitted in their respective failure paths. Rapid double-press in toggle mode and ptt-up-with-no-audio no longer leave the frontend stuck in a phantom recording or transcribing state.

## Context
TurboTalk is a personal-use macOS voice dictation app. The `Recorder` in `src-tauri/src/recorder.rs` implements a 3-state machine: `Ready → Recording → Transcribing → Ready`. The hotkey thread in `src-tauri/src/hotkey.rs` drives transitions on push-to-talk (and on toggle mode).

A multi-agent architecture review identified this state machine as the root cause of the "silent failure" pattern that recurs across the app. Specific bugs:

1. **`recorder.start()` silently returns `Ok(())` when called outside `Ready`** (`recorder.rs:24`). The hotkey emits `ptt-down` to the frontend regardless. The frontend shows "Recording" but no stream is open. Next ptt-up calls `stop()`, which also silently returns `Ok(None)` because state is not `Recording`. No `transcript` or `transcript-error` event fires. The frontend overlay remains stuck on "Transcribing…" forever.

2. **Toggle-mode rapid double-press** (`hotkey.rs:90`): pressing the toggle key during the Transcribing state calls `start()`, which silently fails. `ptt-down` is emitted anyway. UI shows recording. When the prior transcription finishes and returns the recorder to Ready, the next press goes to start() but the UI is already showing recording — visual glitches and lost state.

3. **Paste failures swallowed** (`hotkey.rs:41`): if `paste::paste(&text)` fails, the error is logged but no event is emitted. The `transcript` event is emitted regardless. The user hears the transcript was processed but nothing gets pasted into the focused app, with no UI signal that anything went wrong.

4. **ptt-up with no audio** (`hotkey.rs:55`): when `recorder.stop()` returns `Ok(None)` (silence trim discarded all samples), no event is emitted to the frontend. The overlay's "Transcribing…" stays visible indefinitely.

5. **Fire-and-forget event emission** (throughout `hotkey.rs`): `let _ = app.emit(...)` ignores errors. If a frontend listener isn't registered yet (window closing/opening), events are dropped silently.

The current state machine likely uses an enum like `RecorderState { Ready, Recording, Transcribing }` and matches on it inside `start()`/`stop()`. Adopting a typed-state pattern (different types for different states with `try_*` transitions returning `Result`) is the cleanest fix but may not be necessary — returning `Result<(), RecorderError>` from `start()` and `stop()` and propagating those at the call site is sufficient.

## In scope
- `src-tauri/src/recorder.rs` — change `start()` and `stop()` signatures to return `Result` with a typed error indicating illegal-transition vs underlying-IO-error
- `src-tauri/src/hotkey.rs` — propagate those errors; never emit UI events on failed calls; add `paste-error` and `recording-discarded` events
- `src/App.svelte` — listen for the new events and surface them (paste-error → toast or banner, recording-discarded → reset overlay state)
- `src/Overlay.svelte` — listen for `recording-discarded` and reset to idle (it currently relies on `transcript` or `transcript-error`)

## Out of scope
- Any change to `audio.rs` (covered by TASK-3 / TASK-6)
- Cleanup module logic (TASK-4)
- Adding new hotkeys, modes, or settings
- Refactoring the CGEventTap callback structure beyond what's needed for error propagation
- Frontend visual redesign — keep existing UI patterns; just add the new event handlers

## Steps
1. Read `src-tauri/src/recorder.rs` end-to-end and document the current `start()` and `stop()` behavior, including what they return today.
2. Define a `RecorderError` enum in `recorder.rs`: variants like `IllegalTransition { from: State, attempted: &'static str }`, `Audio(anyhow::Error)`, plus whatever the existing IO-error path returns.
3. Modify `start()`: if state is not `Ready`, return `Err(RecorderError::IllegalTransition { ... })`. If audio open fails, return the audio error. On success, transition to `Recording`.
4. Modify `stop()`: if state is not `Recording`, return `Err(RecorderError::IllegalTransition { ... })`. Otherwise existing behavior — return `Ok(Option<PathBuf>)` where `None` = silence-trimmed.
5. In `hotkey.rs::ptt_down`: call `recorder.start()`. If it returns `Err`, log the error AT WARN LEVEL and DO NOT emit `ptt-down`, DO NOT change tray icon. Just return.
6. In `hotkey.rs::ptt_up`:
   - Call `recorder.stop()`. If it returns `Err`, log at warn, do not emit anything else, return.
   - If it returns `Ok(None)` (silence trimmed), emit a new event `recording-discarded` with no payload, set tray to Idle, return.
   - If it returns `Ok(Some(path))`, proceed to transcription as before.
   - In the spawned transcription thread, after `paste::paste(&text)`: if paste returns `Err`, emit a new event `paste-error` with the error message as payload (and still emit `transcript` with the text — the user should still see the result in history). If paste succeeds, no extra event.
7. Replace `let _ = app.emit(...)` for the new critical events with code that logs at `warn!` level on failure to emit. (Existing fire-and-forget calls can stay if they're for non-critical signals like audio levels.)
8. Update `src/App.svelte`: add `listen('paste-error', ...)` that sets a banner state with the error text — similar to the existing `transcript-error` banner, but distinguishable. Add `listen('recording-discarded', ...)` that does NOT show an error (recording was just too quiet/short — show a brief subtle indicator or do nothing visible, but ensure no transcribing state is left hanging).
9. Update `src/Overlay.svelte`: add `listen('recording-discarded', ...)` that immediately sets `mode = 'idle'` (the same way `transcript` does). This ensures the overlay clears even when no transcript event fires.
10. Run `cargo build --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`.
11. Manually test all four bug scenarios:
    - **Toggle double-press:** in toggle mode, press key, speak briefly, release — wait for the Transcribing state but during it press the toggle key again. Verify the second press is logged as a warning and the UI does not flicker into a phantom recording state.
    - **Silence ptt-up:** press hotkey, do not speak (or whisper below threshold), release. Verify the overlay clears within ~1s instead of hanging on "Transcribing…".
    - **Paste failure:** simulate paste failure (e.g., temporarily edit `paste.rs` to return Err always, or revoke Accessibility permission). Verify a `paste-error` event surfaces in the UI and the transcript still appears in history.
    - **Normal flow:** push, speak "hello world", release. Verify text appears in focused app and history exactly as before — no regression.

## Success signal
- `cargo build` and `cargo clippy -D warnings` exit 0.
- All four test scenarios behave as described.
- A `grep` in `hotkey.rs` shows that `ptt-down` and `ptt-up`/`transcript` are emitted only inside the `Ok` branches of recorder calls.
- Two new events `paste-error` and `recording-discarded` exist (visible in `app.emit("...", ...)` calls in `hotkey.rs`) and have corresponding `listen(...)` handlers in `src/App.svelte` and/or `src/Overlay.svelte`.
- The Recorder's `start()` and `stop()` return `Result` types that include an illegal-transition variant.

## Notes
- Don't try to convert the state machine to typestate (different types per state) — it's overkill for this codebase. A single enum + `Result` return is enough.
- Keep error messages user-readable. The frontend `paste-error` banner should show something like "Couldn't paste — check Accessibility permission" rather than "keystroke failed: -25211".
- Be careful with the Arc<Recorder> lifetimes: the spawned transcription thread takes `recorder.clone()` to keep it alive; that pattern stays.
- Multi-agent review reference: findings ARCH-001, ARCH-002, ARCH-003, ARCH-004, ARCH-015 / MAC-5 in `/tmp/code-analysis-concern-based-main-20260501.md`.
