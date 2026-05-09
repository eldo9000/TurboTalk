# TASK-47: Persistent whisper worker (model warmth)

## Goal

Add a long-lived whisper backend (whisper-server sidecar, or equivalent long-lived process) so subsequent dictations in a session skip model reload — while preserving cancellation, model-change invalidation, one-in-flight semantics, and clean process shutdown on app exit.

## Context

This is the highest-impact architectural win available in the speed pass. Today every dictation spawns `whisper-cli` and reloads the full model. The lifecycle and the per-call spawn live in `src-tauri/src/transcribe.rs:166-205, 221-340, 371-425`. The seam where warmth plugs in already exists:

- `TranscriptionWorker` (struct at `transcribe.rs:166-205`) — already validates model + binary at construction, holds a `spawn_lock` Mutex enforcing one-in-flight.
- `WORKER` static (`transcribe.rs:371`) — process-wide cache, rebuilt on model change.
- `invalidate_worker()` (`transcribe.rs:386-392`) — already wired from `settings::save` so model edits drop the cached worker.
- `abort_active()` (`transcribe.rs:376-381`) — kill path used by `Recorder::cancel` (TASK-23).

Prior attempts (TASK-18, TASK-20) deferred model warmth on 2026-05-02. The two routes attempted:
- **`whisper-rs = "0.16"` (Rust bindings)**: `cargo check` hung 300+ s in `whisper-rs-sys`'s build script, same `cmTC_*` cmake probe symptom as the original TASK-18 deferral. Repro confirmed on this host.
- **`whisper-server` long-lived sidecar**: not bundled in `src-tauri/binaries/`. The deferral note specifically said "downloading external binaries is out of scope for that task" — but it is in scope for *this* task.

The cleanest route is bundling `whisper-server` from the same whisper.cpp build that produced the current `whisper-cli`. Long-lived HTTP server, app POSTs WAV, server returns transcript. Smallest implementation surface; reuses every existing constraint (allowed-roots, model validation).

Tier 1: name the proof. Second-and-later dictations in a session are materially faster than the first; bench numbers captured; cancel + model-change invalidation + orphan-check all pass.

This is the largest task in the speed-pass sprint. If it feels like more than a single session of focused work, **stop and split it before continuing** — don't push through.

## In scope

- `src-tauri/binaries/` — add `whisper-server` sidecar (target-triple suffix convention, e.g. `whisper-server-aarch64-apple-darwin`)
- `src-tauri/src/transcribe.rs` — extend `TranscriptionWorker` to spawn the sidecar in `from_config`, hold its handle, and call into it via HTTP in `transcribe()` instead of re-spawning whisper-cli per call
- `src-tauri/tauri.conf.json` and `src-tauri/tauri.macos.conf.json` — add the sidecar to the bundled binaries / externalBin list
- `src-tauri/src/settings.rs` — verify the model-change invalidation path triggers a worker rebuild (already wired; just confirm)
- App shutdown hook — ensure the sidecar dies when the Tauri app exits

## Out of scope

- CoreML (separate task)
- decode flag tuning (separate tasks)
- model swap (separate task)
- streaming / partial-result features
- adding a `whisper-rs` Rust dependency (the cmake hang is the documented blocker; the `whisper-server` route avoids it entirely)

## Steps

1. Decide implementation route. Default to **Route A: bundle `whisper-server`**. Only deviate if evidence forces it.
   - Route A: bundled sidecar, long-lived HTTP server, per-dictation POST.
   - Route B: long-lived `whisper-cli` wrapper fed via stdin (only if the bundled `whisper-cli` exposes a streaming/batch mode — verify with `--help`).
   - Route C: revisit `whisper-rs` (only if a known cmake-hang workaround is found).
2. (Route A) Build or obtain `whisper-server` matching the bundled `whisper-cli` version. Verify the binary by running it standalone, POSTing a sample WAV, and getting a transcript back.
3. Place the binary in `src-tauri/binaries/whisper-server-aarch64-apple-darwin` (matching the existing whisper-cli naming convention so the same allowed-roots logic applies).
4. Update `src-tauri/tauri.conf.json` `externalBin` list and `src-tauri/tauri.macos.conf.json` resources to ship the new sidecar.
5. Extend `TranscriptionWorker`:
   - In `from_config`, spawn `whisper-server` with the validated model path. Read the chosen ephemeral port from the sidecar's stdout startup line. Store the port and the child handle on the struct.
   - In `transcribe()`, replace the per-call `Command::new(&self.bin).args(&args).spawn()` block (currently `transcribe.rs:272-302`) with an HTTP POST of the WAV to the sidecar's `/inference` endpoint. Keep `spawn_lock` held across the call so one-in-flight semantics are unchanged.
   - Implement `Drop` on `TranscriptionWorker` that kills the sidecar child cleanly. This handles both model-change invalidation (worker dropped → sidecar dies) and process exit.
6. Wire app-shutdown safety: register a Tauri exit handler (or use `Drop` propagation through the static `WORKER`) that ensures the sidecar dies before the parent does. Verify with `pgrep -fl whisper-server` after a clean app quit.
7. Verify cancellation. `Recorder::cancel` mid-transcription must:
   - Interrupt the in-flight HTTP request OR kill the sidecar.
   - Leave the next dictation working (rebuild the sidecar if it was killed).
   The simplest reliable path: on cancel, kill the sidecar; the next dictation will rebuild via the existing `worker_for` cache-miss flow. Acceptable as long as it's not the steady-state path.
8. Verify model-change invalidation. Change the model in Settings UI. Confirm:
   - `[transcribe] worker invalidated` fires.
   - The old sidecar process exits (verify with `pgrep`).
   - The next dictation builds a new worker against the new model and the new sidecar starts cleanly.
9. Bench. Dictate 5 utterances in a fresh session. Capture `[transcribe] whisper took N ms` for all five. The first should be cold (model load + decode); the second through fifth should be materially faster (decode-only, since the model is warm in the sidecar). Record the cold-vs-warm delta as the headline number.
10. Verify no orphan after app exit: quit, run `pgrep -fl whisper-server`, confirm zero processes.
11. Update `SESSION-STATUS.md` and `TRUTH.md` — model warmth changes the answer to "what works end-to-end."

## Success signal

- Second-and-later dictations in a session show measurable wall-time drop vs. the first (the cold-vs-warm delta is captured in logs).
- Model-change in Settings cleanly restarts the sidecar; the next dictation works.
- Cancel-mid-transcription works and the next dictation still works.
- No orphan sidecar process after app exit (`pgrep -fl whisper-server` returns nothing).
- `TRUTH.md` updated to reflect that the warm path is now the steady state.

## Notes

- whisper.cpp's `whisper-server` exposes an OpenAI-compatible `/inference` endpoint. POSTing a multipart form with `file=@audio.wav` and a few standard fields is the simplest integration. The full request shape is in the whisper.cpp `examples/server/README.md`.
- Ephemeral port: read it from the sidecar's stdout startup line rather than hardcoding. Don't pick a fixed port — that breaks if anything else is using it.
- Serialization across calls is already enforced by `spawn_lock: Mutex<()>` — the new code can keep that lock at the same scope. The lock now serializes HTTP calls instead of subprocess spawns; functionally identical.
- The allowed-roots logic in `transcribe.rs:30-60` was written for `whisper-cli` but applies cleanly to `whisper-server` since the sidecar lookup paths are the same. Reuse it; don't add a parallel allow-list.
- Cancel-via-killing-the-sidecar is acceptable in this task. A finer-grained per-request cancel is a follow-up.
- If you implement a `Drop` on `TranscriptionWorker`, be aware the worker is held in a `static Mutex<Option<Arc<...>>>`. The Arc semantics mean the Drop only fires when the Arc count hits zero — confirm by tracing that no other Arc clone is held across model invalidation.
