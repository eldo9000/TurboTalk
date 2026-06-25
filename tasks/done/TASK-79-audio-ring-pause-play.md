# TASK-79: Audio thread lock-free ring buffer + stream pause/play

## Goal
Replace the `Mutex<Vec<f32>>` on the CoreAudio callback thread with a lock-free SPSC ring buffer, and use `Stream::pause()`/`Stream::play()` instead of keeping the callback running at ~100 Hz during 45-second idle windows.

## Context
Two issues in `src-tauri/src/audio.rs`:

**Issue 1 — Mutex on the audio callback thread (`:588-652`).**
The cpal audio callback acquires `parking_lot::Mutex` on the `samples` Vec (`:597` `smp.lock().extend_from_slice(data)`). A realtime audio callback must never block on a mutex — if the watchdog or `start()` holds the lock, the callback stalls and audio glitches. Additionally, `Vec::extend_from_slice` can trigger a `realloc`/`mmap` on the audio thread when the Vec crosses a capacity boundary.

The code acknowledges this in a comment at `:573-575`: "If profiling shows contention, swap for an SPSC ring (e.g. `rtrb`) — don't pre-optimize." The audit found this is not a theoretical concern — the lock IS contended (the idle watchdog, `start()`, and `stop()` all acquire it).

**Issue 2 — Warm stream stays playing forever (`:655-660`).**
The "warm stream" design keeps the cpal stream in `play()` state forever and gates capture via the `is_recording` atomic inside the callback. This means the callback runs at ~100 Hz for the entire idle window (45 seconds default), doing `push_preroll` + `lvl.store(0)` on every tick. The documented-correct pattern is `Stream::pause()` after recording + `Stream::play()` on the next start.

The `rtrb` crate (lock-free single-producer single-consumer ring buffer) is the standard Rust choice for realtime audio. The producer end lives on the callback thread (lock-free push), the consumer end lives on the feeder thread (drain). No mutex, no allocation on the audio thread (fixed-capacity ring).

## In scope
- `src-tauri/src/audio.rs` — the cpal callback, `start()`, `stop()`, the idle watchdog, and the `samples` data structure
- `src-tauri/Cargo.toml` — add `rtrb` dependency
- `SESSION-STATUS.md`

## Out of scope
- The `audio_finalizer.rs` streaming path (it consumes from the feeder, not from the cpal callback directly — it should be unaffected by the ring buffer change as long as the feeder channel contract stays the same)
- The VAD / resampling logic (those operate on already-captured samples, not on the callback)
- The `callback_scratch` mutex (separate concern — the I16/U16 sample-format conversion buffer; also on the audio thread but a smaller issue)
- The `unsafe impl Send/Sync for AudioCapture` block (the safety argument stays the same — the ring buffer is `Send` natively)

## Steps
1. Read `src-tauri/src/audio.rs` completely, focusing on:
   - The `samples` field: `Arc<Mutex<Vec<f32>>>` (`:609` etc.)
   - The cpal callback at `:588-652`: acquires the mutex, extends the Vec with converted samples
   - `start()` at `:692-808`: clears samples, splices preroll, sets `is_recording`, spawns feeder
   - `stop()` at `:1010-1080`: clones samples for batch fallback, clears
   - The idle watchdog at `:417-467`: polls every 1s
   - The preroll ring (`:588-652`): when not recording, callback pushes to preroll instead of samples
2. Add `rtrb = "0.3"` to `src-tauri/Cargo.toml` dependencies.
3. Replace `samples: Arc<Mutex<Vec<f32>>>` with `samples_producer: rtrb::Producer<f32>` and `samples_consumer: rtrb::Consumer<f32>` (or wrap in a struct that holds both). The ring should be sized for the maximum expected recording length (or a generous fixed cap — e.g. 60 seconds at 24kHz = ~1.4M samples × 4 bytes = ~5.6MB; that's fine for a fixed allocation).
4. In the cpal callback, replace `smp.lock().extend_from_slice(data)` with `producer.write_chunk(len)` which returns a mutable slice into the ring — no allocation, no lock. If the ring is full (feeder fell behind), drop the oldest samples or overwrite (decide based on the existing drop policy — the codebase already drops chunks on the worker side when `CHANNEL_DEPTH` is full, so dropping on the ring side is consistent).
5. In `start()`, instead of clearing the Vec, clear the ring (the consumer side can `clear()`). The preroll splice becomes a push to the producer.
6. In `stop()`, instead of cloning the Vec, drain the consumer into the batch-fallback buffer. No clone needed — the consumer owns the data.
7. For the warm-stream pause/play:
   - After `stop()` completes and the batch fallback / streaming finalizer has drained the ring, call `stream.pause()` (cpal `StreamTrait::pause()`).
   - In `start()`, call `stream.play()` before setting `is_recording = true`. This wakes the callback.
   - The preroll ring is preserved across pause/play (the ring buffer is a plain struct, not tied to the stream state).
   - Update the idle watchdog: when the stream is paused, the watchdog has nothing to do (the stream is already stopped). The 45-second idle close can become a 0-second close (pause immediately after recording stops) OR keep the 45-second grace period but pause the stream. Decide: the 45-second grace exists to avoid stream re-creation latency on rapid presses. `pause()`/`play()` is cheap (no re-creation), so the grace period can be reduced or eliminated. A reasonable approach: pause immediately, keep the stream handle warm, and `play()` on next start. No 45-second timer needed.
8. Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy`.
9. Run `npm run typecheck`.
10. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass.
- `grep -n "Mutex" src-tauri/src/audio.rs` shows no `Mutex<Vec<f32>>` on the `samples` field (the ring buffer replaces it). The `callback_scratch` mutex may remain (separate concern) or may also be replaced — either is acceptable for this task.
- The cpal callback does not acquire any mutex and does not allocate (the `rtrb::Producer::write_chunk` returns a mutable slice into pre-allocated memory).
- The stream is `pause()`d after recording stops and `play()`d on next start. The callback does not run during idle.
- The idle watchdog either runs with a much longer interval (if kept for safety) or is removed (if `pause`/`play` is proven reliable).
- Recording still works: hold Right Alt → speak → release → transcript pasted. The preroll is preserved (leading words not cut off).
- The macOS mic indicator (orange dot in the menu bar) should NOT be lit during idle (because the stream is paused, not running).

## Notes
- `rtrb` is the standard Rust lock-free SPSC ring for audio. It's used by `cpal` examples and major Rust audio projects. `Producer::write_chunk(n)` returns `&mut [f32]` into the ring — you write directly into it, no copy, no allocation. `Consumer::read_chunk(n)` returns `&[f32]` — same, no copy.
- If the ring is full (producer can't write), `write_chunk` returns 0 available slots. Decide: drop the incoming chunk (audio glitch but no stall) or overwrite the oldest data. The codebase already drops on the worker side, so dropping on the producer side is consistent. Log a warning if drops happen.
- `Stream::pause()` on macOS CoreAudio calls `AudioDeviceStop` — this stops the audio HAL callback pump. The mic indicator clears. `Stream::play()` calls `AudioDeviceStart` — resumes. This is the documented CoreAudio lifecycle.
- The preroll ring (the 300ms pre-roll buffer for leading-word preservation) must survive across pause/play. Since it's a separate `Mutex<Vec<f32>>` or similar, it's fine — it's not tied to the stream state. With the ring buffer, the preroll can also be part of the ring (the consumer just doesn't drain the last N samples).
- Be careful with the `is_recording` atomic and the pause/play ordering: `play()` → set `is_recording = true` on start, set `is_recording = false` → drain → `pause()` on stop. The `is_recording` gate inside the callback still matters (it prevents pushing to the main ring during the pre-record preroll phase).
- The `unsafe impl Send/Sync for AudioCapture` block at `:198-245` may need updating — the `rtrb` Producer and Consumer are `Send` natively (no unsafe needed for the ring itself). The `cpal::Stream` is still `!Send`, so the unsafe impl for `ActiveStream` stays.
