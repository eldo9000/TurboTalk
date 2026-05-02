# TASK-3: Audit and prove (or remove) `unsafe impl Send/Sync` on AudioCapture

## Goal
Either:
(a) the `unsafe impl Send` and `unsafe impl Sync` blocks in `src-tauri/src/audio.rs` carry a `// SAFETY:` comment that names exactly which fields are accessed from which threads, under which synchronization, and proves that no data race is reachable; OR
(b) the unsafe impls are removed and the audio module is restructured so the compiler accepts the types without manual unsafe markers.

A reader of `audio.rs` can determine, from the code alone, why concurrent access to `AudioCapture` is sound on macOS.

## Context
TurboTalk is a personal-use macOS voice dictation app. The audio module (`src-tauri/src/audio.rs`) wraps `cpal::Stream` for microphone capture. The current code declares:

```rust
unsafe impl Send for AudioCapture {}
unsafe impl Sync for AudioCapture {}
unsafe impl Send for ActiveStream {}
```

A multi-agent review flagged this as the highest-confidence soundness concern in the codebase. The reasoning:

1. `cpal::Stream` is documented as not `Send` on macOS — its drop must run on the thread that created it (CoreAudio threading model).
2. `AudioCapture` is shared across threads via `Arc<Recorder>` (created in `lib.rs`, used by the hotkey thread and the level-broadcast thread).
3. The unsafe impl bypasses Rust's compile-time check, but the actual safety depends on:
   - The `Mutex<Option<ActiveStream>>` field correctly serializing access to the stream.
   - The CoreAudio callback thread only touching the atomics (`is_recording`, `level`) and the `samples: Arc<Mutex<Vec<f32>>>` — never the `active` field.
   - `start()`, `stop()`, and `Drop` running on the same logical owner thread (or being serialized via the Mutex such that the Stream's `Drop` always runs in a controlled location).

The current code does not document these invariants. They may or may not actually hold — that is what this audit determines.

The threads that touch `AudioCapture`:
- **Hotkey thread** (CGEventTap callback in `hotkey.rs`) — calls `recorder.start()` and `recorder.stop()`.
- **Level-broadcast thread** (spawned in `lib.rs`) — calls `recorder.is_recording()` and `recorder.level()` every 50ms.
- **CoreAudio callback thread** (cpal-managed) — pushes samples into `samples` and updates `level`.

`recorder.start()` calls into `audio.rs` to open a new stream (creates the `cpal::Stream`). `recorder.stop()` takes the active stream out and finalizes a WAV. The stream's `Drop` runs wherever `stop()` runs — that is the hotkey thread, not the thread that created it. **This is the load-bearing question: does cpal on macOS actually require the stream to be dropped on the creating thread, and if so, is that ever violated?**

## In scope
- `src-tauri/src/audio.rs` — the unsafe impls and the synchronization protocol they rely on
- `src-tauri/src/recorder.rs` — only as needed to confirm what threads call into AudioCapture
- A `// SAFETY:` comment block on each unsafe impl OR a redesigned module that doesn't need unsafe
- `cpal` documentation review (read `cargo doc --open -p cpal` or the upstream README)

## Out of scope
- Other concurrency in the codebase (hotkey state, history saves, etc.)
- Changing the public API of `Recorder` in lib.rs — the rest of the app should be unaffected
- Performance tuning of the audio path
- Adding new features (silence trim, VAD, etc.)
- Other unsafe blocks in the codebase (`hotkey.rs:123` is a separate task)

## Steps
1. Read `src-tauri/src/audio.rs` end to end. Map every field of `AudioCapture` and `ActiveStream` and note whether it is touched by: (a) the calling thread of `start`/`stop`, (b) the level/is_recording readers, (c) the CoreAudio callback thread.
2. Read `src-tauri/src/recorder.rs` and confirm the call patterns: which functions are called from which threads in `lib.rs` and `hotkey.rs`.
3. Read cpal's documentation on `Stream` thread-safety. Look for: (a) is `Stream: !Send` on macOS specifically, (b) what happens on Drop, (c) any documented requirement that Drop run on a specific thread. Cite the source you read.
4. Determine which case applies:
   - **Case A — provably safe as written:** every thread access is either through an atomic or through a Mutex, the Stream is only ever touched while the Mutex is held, and cpal does not require Drop on a specific thread. Document this with a SAFETY comment on each unsafe impl that names the specific synchronization (e.g., `SAFETY: ActiveStream is only ever accessed while the parent AudioCapture's `active` Mutex is held, and cpal::Stream::drop on macOS is safe to run from any thread because <reason>`).
   - **Case B — currently unsound or unprovable:** the SAFETY argument cannot be made. Redesign: move stream ownership to a dedicated audio thread that owns the Stream for its full lifetime. Communicate with it via channels (`std::sync::mpsc` or `crossbeam::channel`). The audio thread receives `Start`/`Stop` commands and replies with results. AudioCapture becomes a thin handle holding the channel ends — those are naturally Send + Sync without unsafe.
5. If Case A: add the SAFETY comments. If Case B: do the refactor; remove all three `unsafe impl` lines; verify the code compiles without them.
6. Run `cargo build --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`.
7. Manually test: launch with `npm run tauri dev`. Hold the PTT hotkey (Right Option by default), speak, release. Confirm transcription end-to-end works. Repeat 5+ times back-to-back rapidly to stress the start/stop cycle. Then do a long recording (15+ seconds) to stress the callback thread.
8. Observe: no panics, no audible glitches, transcription quality unchanged. Add `tracing::info!` at start/stop if needed to confirm the lifecycle is clean.

## Success signal
- Either every `unsafe impl` in `audio.rs` has a SAFETY comment that names the specific synchronization argument, OR the unsafe impls are gone and the file compiles cleanly without them.
- `cargo build` and `cargo clippy -D warnings` exit 0.
- 5 consecutive PTT cycles followed by one 15-second recording all succeed without panic or hang. Transcripts appear correctly in the history tab.
- A reviewer can read `audio.rs` and answer "why is concurrent access sound here?" using only what's in the file.

## Notes
- The cpal version is in `src-tauri/Cargo.toml`. Pin the docs you read to that version (`cargo doc --open -p cpal`).
- If you choose Case B (refactor), keep the public API of `AudioCapture` identical — `Recorder` should not need to change. Internal wiring only.
- This task blocks any future concurrency changes (background cleanup, async transcription, etc.). Do not attempt those changes until this one lands.
- Multi-agent review reference: findings SEC-011, ARCH-006 / MAC-1 in `/tmp/code-analysis-concern-based-main-20260501.md`.
