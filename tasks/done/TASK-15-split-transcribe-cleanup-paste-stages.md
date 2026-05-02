# TASK-15: Split transcribe, cleanup, and paste into named stages with job ids

## Goal
The post-audio pipeline is stage-addressable and observable:

`Transcribing → Cleaning → Pasting`

Each dictation job has a `job_id` attached to backend logs and frontend events so future queueing, cancellation, or paste-target work has a clean foundation.

## Context
Today `transcribe.rs::run()` spawns Whisper and then immediately calls `cleanup::process(...)` before returning. `hotkey.rs` then emits `transcript` and calls `paste::paste(...)`.

That works, but it hides the difference between Whisper latency, cleanup latency, and paste latency. It also makes the lifecycle states from TASK-14 less meaningful because `Transcribing` secretly includes cleanup.

## In scope
- `src-tauri/src/transcribe.rs`
  - Change the main transcription function so it returns raw Whisper text only.
  - Keep Whisper path validation and output-file handling unchanged.
- `src-tauri/src/hotkey.rs`
  - Call cleanup as its own explicit stage after raw transcription.
  - Call paste as its own explicit stage after cleanup.
  - Attach a monotonically increasing `job_id` to logs and events where useful.
- `src-tauri/src/cleanup.rs`
  - No behavior changes expected, only call-site changes.
- Frontend listeners may ignore `job_id` initially, but event payloads should be forward-compatible.

## Out of scope
- Changing cleanup rules.
- Changing Whisper flags.
- Persistent Whisper.
- Queued dictation.
- Paste-target capture.

## Steps
1. Read `src-tauri/src/transcribe.rs::run`, `src-tauri/src/cleanup.rs::process`, and `src-tauri/src/hotkey.rs`.
2. Rename or split `transcribe::run`:
   - Preferred: `transcribe::run_raw(wav: &Path) -> anyhow::Result<String>`.
   - Keep a compatibility wrapper only if it avoids too much churn.
3. Ensure `run_raw` trims Whisper's `.txt` output but does not call `cleanup::process`.
4. In `hotkey.rs`, after `run_raw` succeeds:
   - transition to `Cleaning`.
   - call `cleanup::process(&raw_text)`.
   - transition to `Pasting`.
   - emit final transcript and paste.
5. Add a backend job id:
   - `AtomicU64` is enough.
   - Increment when a recording successfully starts.
   - Include `job_id` in important logs.
6. Decide event payload shape:
   - For existing frontend compatibility, either keep `transcript` as a string or update listeners carefully.
   - If changing payloads, update `src/App.svelte` and `src/Overlay.svelte` in the same task.
   - A safe option is to keep existing events unchanged and add new `dictation-stage` events with `{ job_id, stage }`.
7. Emit `dictation-stage` events for:
   - `recording`
   - `finalizing_audio`
   - `transcribing`
   - `cleaning`
   - `pasting`
   - `ready`
8. Add or update tests for `transcribe.rs` path validation if function names changed.
9. Run:
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
10. Manual test normal dictation and confirm history/paste behavior is unchanged.

## Success signal
- Raw transcription and cleanup are separate call sites.
- Logs or events show distinct `transcribing`, `cleaning`, and `pasting` stages for one job.
- Existing UI behavior still works.
- Tests and clippy pass.

## Notes
- Prefer additive `dictation-stage` events over breaking the existing `transcript` event shape unless there is a strong reason.
- This task prepares the ground for persistent Whisper and future queueing.

