# TASK-17: Cache or reuse the Silero VAD session if timing evidence justifies it

## Goal
Silero VAD initialization is not paid unnecessarily on every recording. If TASK-13 timing shows meaningful VAD setup cost, reuse the model/session safely across dictation jobs.

## Context
`src-tauri/src/vad.rs` currently embeds the ONNX model bytes and materializes them to a temp path once per process, but `trim()` constructs a fresh `SmoothedVad` / `Vad` for each recording.

That may be fine if init is cheap. It may be wasteful if ONNX session creation is a noticeable part of post-release latency.

## In scope
- Measure or use TASK-13 timings to decide whether VAD session reuse is worth implementing.
- If worth it, implement a reusable VAD worker/session.
- Preserve the smoothing behavior:
  - onset
  - prefill
  - hangover
  - graceful fallback to full-buffer trim on VAD failure
- Add tests for repeated calls so state does not leak between recordings.

## Out of scope
- Changing VAD constants.
- Streaming VAD.
- Moving VAD before resampling.
- Replacing Silero.

## Steps
1. Read `src-tauri/src/vad.rs` end-to-end.
2. Review TASK-13 timing logs for VAD stage cost. If timing evidence does not separate model/session init from per-frame compute, add temporary or permanent timing inside `vad.rs`.
3. Determine whether `vad_rs::Vad` supports safe reuse with reset semantics.
   - If the library has an explicit reset method, use it.
   - If not, prefer a small pool or cached initialized resources only where safe.
   - Do not reuse a stateful VAD object if prior audio can influence the next recording.
4. Implement one of:
   - reusable `Vad` guarded by `Mutex`, with per-call smoothing state reset; or
   - a dedicated VAD worker thread that receives complete 16 kHz mono buffers and returns trim bounds; or
   - no behavior change plus a note in the task file if measured init cost is negligible.
5. Add repeated-call tests:
   - Call `trim()` twice with different buffers.
   - Assert the second call does not inherit speech bounds from the first.
   - Keep synthetic speech tests ignored if Silero remains unreliable on non-human signals; focus on state isolation and fallback shape.
6. Run:
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
7. Manual test several back-to-back recordings and compare timing logs before/after.

## Success signal
- Either VAD session reuse is implemented safely, or timing proves it is not worth touching yet and that decision is recorded.
- No state leaks between recordings.
- Dictation quality does not regress at word boundaries.
- Tests and clippy pass.

## Notes
- Do not move silence trimming earlier in the pipeline. Silero wants 16 kHz mono f32, so the current stage is correct.
- If reuse is unsafe because the library is stateful and lacks reset, keep correctness over speed.

