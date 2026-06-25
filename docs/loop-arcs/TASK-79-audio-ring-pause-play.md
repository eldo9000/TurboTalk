# Arc Log — TASK-79: Audio thread lock-free ring buffer + stream pause/play

## Gate
Replace `Mutex<Vec<f32>>` on the CoreAudio callback with a lock-free SPSC ring
buffer (`rtrb`), and use `Stream::pause()` / `Stream::play()` instead of keeping
the callback running during idle.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** The cpal callback acquires `parking_lot::Mutex` on `samples` Vec
  and can trigger `realloc` via `extend_from_slice`. The stream stays in `play()`
  state during idle (~45s), running the callback at ~100 Hz for nothing.
- **PREMISE:** Replacing the `Mutex<Vec<f32>>` with `rtrb::Producer` (lock-free push,
  no allocation) and using `Stream::pause()`/`play()` to gate the callback will
  eliminate both the mutex contention on the audio thread and the idle callback spam.
- **DERIVATION:** `rtrb` is the standard lock-free SPSC for audio in Rust.
  `Stream::pause()` calls `AudioDeviceStop` — the HAL callback pump stops entirely.
  `Stream::play()` calls `AudioDeviceStart` — resumes. No re-creation needed.
- **FALSIFICATION:** If `cargo check` fails (rtrb API mismatch, type errors across
  AudioCapture struct, feeder, stop()) the premise is false.
- **FALSIF-RESULT:** `cargo check` + `cargo clippy` clean. Callback uses `push_partial_slice` (no realloc). `Stream::pause()`/`play()` added. 135/138 tests pass (2 pre-existing transcribe garbage-detection failures).
- **DISPOSITION:** CONFIRMED — dispatch 1 green. Commit ccce04c.
