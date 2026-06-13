# TASK-3: whisper-server crash recovery + from_config sleep optimization

## Goal
If the whisper-server child process crashes or becomes unreachable, the transcription system automatically detects the failure and re-warms a new worker, instead of permanently failing every subsequent dictation. Also, the flat 500ms sleep in `from_config` is replaced with a responsive readiness poll.

## Context

### Crash recovery (transcribe.rs:1009-1045)
The whisper-server runs as a child process. If it dies on its own (OOM on a big model, GGML assertion, etc.), the `READY` flag stays `true` and the `WORKER` static still caches the dead backend. Every subsequent dictation: POST fails → `transcript-error` → done. Nothing invalidates the worker. Only escape: restart app or change model.

The cancel path already has a rewarm pattern (`trigger_cancel` re-warms when `!is_ready()`). The error path needs the same: in `run_raw`/`transcribe`, when the POST fails with a connection-refused, connection-reset, or broken-pipe error, call `invalidate_worker()` to clear the dead cache and then call `prewarm()` to start a fresh server. Optionally retry the request once inline after rewarming.

### Flat sleep (transcribe.rs:795)
`from_config` has a hard `thread::sleep(Duration::from_millis(500))` before the readiness poll. Every server build — including post-cancel rewarms — eats this 500ms. Replace with a `try_wait` loop: attempt a health-check connection every 50ms for up to 500ms, breaking early when the server responds. The readiness poll that follows already covers the rest.

## In scope
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/transcribe.rs`

## Out of scope
- Any other files
- The transcribe backends subdirectory (`transcribe_backends/`)
- Ollama backend (`ollama.rs`)
- Audio pipeline, recorder, hotkey

## Steps
1. Read `transcribe.rs` fully. Locate:
   a. The `invalidate_worker()` function or equivalent that clears/restarts a dead worker.
   b. The `prewarm()` function.
   c. The `run_raw` and/or `transcribe` function where the HTTP POST to the whisper-server happens (~line 1009-1045).
   d. The `from_config` function, specifically the `thread::sleep(Duration::from_millis(500))` (~line 795).
   e. The `trigger_cancel` function (for the rewarm pattern reference).
2. **Fix crash recovery**: In the error-handling path of `run_raw`/`transcribe` where the POST fails:
   - Check if the error is a connection-level error (connection refused, connection reset, broken pipe, timeout).
   - If so, call `invalidate_worker()` to clear the dead backend cache.
   - Call `prewarm()` to start a fresh server.
   - Optionally retry the request once after prewarm succeeds.
   - If the retry also fails, propagate the error as before (don't loop forever).
3. **Fix flat sleep**: In `from_config`, replace the flat 500ms sleep with a loop that:
   - Sleeps 50ms per iteration.
   - After each sleep, attempts a quick health check (e.g., TCP connect to the server port, or the existing readiness endpoint).
   - Breaks early if the server responds.
   - Has a maximum of 10 iterations (500ms total), matching the current behavior as a fallback.
4. Run `cargo check` in `src-tauri/` to verify compilation.

## Success signal
`cargo check` passes. The transcription error path includes `invalidate_worker()` + `prewarm()` on connection-level failures. `from_config` uses a 50ms polling loop instead of a flat 500ms sleep.

## Notes
- The exact error types depend on the HTTP client used (likely `ureq` or `reqwest`). Match on transport-level errors, not HTTP status codes.
- The rewarm pattern in `trigger_cancel` is the reference for how to call `invalidate_worker` + `prewarm`.
- If the retry after rewarm approach adds too much complexity to `run_raw`, a simpler version that just invalidates + prewarms (without inline retry) still closes the loop — the user's next press will use the fresh worker.
