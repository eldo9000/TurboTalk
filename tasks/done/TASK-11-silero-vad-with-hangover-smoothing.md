# TASK-11: Replace RMS silence trimmer with Silero VAD + hangover smoothing

## Goal
The recorded buffer is trimmed using **Silero VAD** with prefill, onset, and hangover smoothing rather than the existing fixed-threshold RMS gate. Leading and trailing silence/garbage is removed without clipping word boundaries. Push-to-talk semantics are unchanged — the hotkey still drives recording start/stop; VAD only refines where speech actually begins and ends inside the captured buffer.

## Context
TurboTalk is a personal-use macOS dictation app. The existing silence trimmer (`trim_silence` in `src-tauri/src/audio.rs`) uses a fixed RMS threshold of -42 dBFS over 20 ms windows. It's primitive: it misfires on any background noise above the threshold (HVAC, fan, distant voices) and clips word edges on quiet consonants. Whisper hallucinates ("um", " thanks for watching", " you") on the small amounts of leading/trailing dead air that survive the RMS gate.

The fix is to swap to a neural VAD (Silero, ~2 MB ONNX model) with hangover smoothing — the standard recipe used by reference dictation apps like cjpais/Handy. Reference implementation files:
- `/tmp/handy-ref/src-tauri/src/audio_toolkit/vad/silero.rs`
- `/tmp/handy-ref/src-tauri/src/audio_toolkit/vad/smoothed.rs`

Handy's tuned constants (use these — don't invent new ones):
- `prefill_frames: 15` — frames to keep BEFORE the first onset
- `onset_frames: 2` — consecutive speech frames required to trigger "onset"
- `hangover_frames: 15` — frames of silence required to trigger "offset"
- `threshold: 0.3` — Silero probability threshold

Silero v4 expects 16 kHz mono f32 in 30 ms frames (480 samples).

## In scope
- `src-tauri/Cargo.toml` — add `vad-rs` (the crate Handy uses) plus its required onnxruntime dependency.
- New file `src-tauri/src/vad.rs` containing:
  - The `SmoothedVad` struct (or equivalent) wrapping the Silero session and the smoothing state machine.
  - A public function `pub fn trim(samples: &[f32]) -> (usize, usize)` returning the start/end indices of speech, or `(0, samples.len())` on VAD failure (graceful fallback).
- `src-tauri/src/audio.rs::stop()` — replace the call to `trim_silence` with `vad::trim(&buf)`. Keep the existing `min_samples` rejection (recordings under 100 ms are still discarded).
- The Silero ONNX model file. Two options:
  - **Preferred:** `include_bytes!("../resources/silero_vad_v4.onnx")` so the model is embedded in the binary. Add the file under `src-tauri/resources/`.
  - **Alternative:** declare it as a Tauri resource in `tauri.conf.json` and load via `app.path().resolve(...)`. Slightly more complex; only do this if `include_bytes!` blows up the binary size unacceptably (it adds ~2 MB).

## Out of scope
- Real-time / streaming VAD. The recording is finished by the time `vad::trim` runs — operating on the complete buffer is fine.
- Replacing the push-to-talk hotkey with VAD-only auto-detection — that's a separate UX decision.
- Any change to `hotkey.rs`, `recorder.rs`, or the frontend.
- Removing `trim_silence` itself — leave the function in place but make it dead code (or delete if confident nothing else calls it). Verify with `grep`.
- Multilingual VAD models (Silero is language-agnostic; we don't need a different one).

## Dependencies
- This task **must land after TASK-9** (resample to 16 kHz mono). Silero expects 16 kHz mono f32; running it on 48 kHz stereo gives garbage.
- Independent of TASK-10 (normalize) and TASK-12 (whisper flags). Order between TASK-10 and TASK-11 doesn't matter — both touch different parts of `stop()`. If TASK-10 is merged first, normalize the buffer before running VAD on it (slightly better for quiet speech detection). If TASK-11 is merged first, that's also fine.

## Steps
1. Read `/tmp/handy-ref/src-tauri/src/audio_toolkit/vad/silero.rs` and `smoothed.rs` end-to-end. The smoothed-state machine logic is the load-bearing part — copy the pattern, don't reinvent.
2. Pick the VAD crate. Check Handy's `Cargo.toml` to see exactly which crate + version they use (likely `vad-rs` or similar wrapping `ort`). Verify it builds on macOS with the existing toolchain.
3. Add the chosen crate(s) to `src-tauri/Cargo.toml`. If the crate brings in `ort` (onnxruntime), check that it doesn't require a system-installed onnxruntime — we want fully self-contained binaries. Look for a `download-binaries` or `bundled` feature flag.
4. Download the Silero VAD v4 model from the official Silero release (https://github.com/snakers4/silero-vad). Save it as `src-tauri/resources/silero_vad_v4.onnx`. Add the resources dir to git (and `.gitignore` only the build outputs, not the model itself).
5. Create `src-tauri/src/vad.rs`:
   - `static MODEL_BYTES: &[u8] = include_bytes!("../resources/silero_vad_v4.onnx");`
   - Lazy-init a thread-safe `Session` on first use (`OnceCell` or `Lazy`).
   - `pub struct SmoothedVad { /* session, prefill_buf, hangover_counter, onset_counter, in_speech: bool */ }`
   - `pub fn trim(samples: &[f32]) -> (usize, usize)` — instantiate a `SmoothedVad`, feed it 480-sample frames, run the state machine, return the index range of detected speech. Pad the final partial frame with zeros.
   - On any error (model failed to load, ONNX inference failed) return `(0, samples.len())` as a graceful fallback. Log a `tracing::warn!`.
6. Register the module in `src-tauri/src/lib.rs`: `pub mod vad;`.
7. In `src-tauri/src/audio.rs::stop()`:
   - Replace `let (start, end) = match trim_silence(&buf, sample_rate) { ... };` with `let (start, end) = vad::trim(&buf);`.
   - Keep the `min_samples` check that follows (under 100 ms = silence-discarded).
8. Run `cargo build --manifest-path src-tauri/Cargo.toml`. The binary size will increase by ~2 MB (Silero model) plus whatever `ort`/onnxruntime adds (~10 MB) — confirm this is acceptable.
9. Run `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and `cargo test`.
10. Add a unit test in `vad.rs`:
    - Synthetic input: 1 s of silence (zeros) + 1 s of a 440 Hz sine wave at amplitude 0.3 + 1 s of silence, at 16 kHz mono.
    - Assert `trim(...)` returns `(start, end)` where `start` is in the 1.0s region (within ±0.2 s) and `end` is in the 2.0s region (within ±0.2 s).
11. Manually test:
    - PTT, then 1 s of silence, "hello world", 1 s of silence, release.
    - Verify the trimmed audio (intercept the WAV via tracing logs) has the silence trimmed but "hello world" intact at the boundaries.
    - Compare whisper output with VAD on vs off — expect leading/trailing hallucinations to disappear.
    - Test edge case: PTT, no speech, immediate release. Verify the existing `recording-discarded` event still fires (because `min_samples` still rejects under-100ms buffers).

## Success signal
- The unit test passes.
- Real-world trimming preserves word boundaries (no audible clipping at "h" of "hello" or "d" of "world").
- Whisper transcript no longer contains leading/trailing hallucinations on tested recordings.
- Binary size increase is bounded (≤15 MB total addition for ort + model).
- `cargo build`, `cargo clippy -- -D warnings`, `cargo test` exit 0.

## Notes
- Don't tune the smoothing constants from scratch. Use Handy's values verbatim: prefill=15, onset=2, hangover=15, threshold=0.3. They're tuned against real dictation audio.
- Silero is language-agnostic — works for English-only and multilingual models alike.
- If `vad-rs` requires a system onnxruntime, switch to whatever crate provides bundled inference (e.g., `ort` with `download-binaries` feature). Self-contained is non-negotiable; this is a personal app shipped as a Tauri bundle, not a CLI tool.
- The fallback path (return `(0, samples.len())` on VAD failure) means we degrade gracefully to "no trimming" — never to a crash and never to dropping the entire recording.
- Verify after this task that `audio.rs::trim_silence` is unreferenced anywhere. If so, delete it (and its unit tests if any).
