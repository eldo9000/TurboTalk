# TASK-37: Pre-roll ring buffer for first-words capture

## Goal
When the user starts speaking *before* (or simultaneously with) the PTT key registers, the first ~300 ms of audio is captured and prepended to the recording, so leading words are never clipped — even on the first press of a session.

## Context
TASK-36 (must already be merged) keeps the cpal stream warm between recordings. With a warm stream, audio is flowing into the cpal callback continuously, but samples are only kept while `is_recording=true`.

Pre-roll: while the stream is warm and `is_recording=false`, write incoming samples into a small ring buffer. When PTT-down fires, copy the ring buffer's contents into the per-recording `samples` buffer first, then continue accumulating new samples on top. The ring is a fixed-size `VecDeque<f32>` of native-rate samples (or a manual circular buffer); size = `PREROLL_MS * native_sample_rate / 1000`.

Rationale: humans often start a word a few hundred ms before they finish pressing the modifier key. Even with TASK-36's instant capture, the very first phoneme can land in the gap between "user begins speaking" and "key registers". A pre-roll fixes this in the common case where the stream is already warm.

When the stream is NOT warm (cold first press of a session), pre-roll is empty — no rescue available. That's fine; users who care about every-press perfection will see the warm-stream path 99% of the time.

300 ms is the recommended starting size. Long enough to catch most leading-word clip-offs (a typical word is 150–400 ms), short enough that you don't accidentally include a prior cough or click. Pin it as a constant; no user-facing setting.

The ring buffer must:
- Be filled by the cpal callback thread (high-priority CoreAudio thread). Operations there must be O(1) and lock-free or use a very short critical section. A `Mutex<VecDeque<f32>>` is acceptable if `extend` + `truncate-from-front` is done under one lock and the lock is uncontended in practice.
- Be drained by the hotkey worker thread inside `start()` after `is_recording.store(true)`. Drain order: oldest-first (so when prepended to `samples`, time ordering is preserved).
- Be resilient to channels: the cpal callback delivers interleaved multi-channel samples. The ring stores raw native-rate, native-channel samples — same format as today's `samples` buffer mid-recording — so no per-callback DSP is added. Downmix happens later in `stop()` against the combined buffer (pre-roll + recording), as it does today.

The size of the ring in samples = `300 ms * sample_rate / 1000 * channels`. Allocate once on stream open; never re-allocate.

Edge cases:
- First press on a cold stream: ring is empty (or has fewer than 300 ms). Use whatever's there.
- Stream rebuild on device change (TASK-36): drop the old ring, allocate a new one sized for the new sample rate / channels.
- The ring operates regardless of `is_recording` — it's always fed by the callback. Bounded size guarantees memory cost is fixed (~57 KB at 48 kHz mono f32).

## In scope
- `src-tauri/src/audio.rs` — add ring buffer, modify cpal callback, modify `start()` to drain ring on PTT-down

## Out of scope
- `src-tauri/src/recorder.rs` — no changes
- `src-tauri/src/audio_finalizer.rs` — no changes; the streaming finalizer just sees a slightly longer initial chunk
- `src-tauri/src/hotkey.rs` — no changes
- Frontend / overlay — no changes
- A user-facing setting for pre-roll length — keep as a hardcoded constant
- Pre-roll across stream rebuilds (a brand-new device gets no pre-roll — accepted)

## Steps
1. Confirm TASK-36 is merged on the current branch. If not, stop and tell the user.
2. Add to `AudioCapture`:
   - `preroll: Arc<Mutex<VecDeque<f32>>>` (or a fixed-size circular buffer struct).
   - `preroll_capacity: AtomicUsize` — capacity in samples; set when stream is opened so the cpal callback can read it without locking.
3. Define `const PREROLL_MS: u32 = 300;`.
4. In `open_stream()` (or wherever the stream is built post-TASK-36), after determining `sample_rate` and `channels`, compute `preroll_capacity = (PREROLL_MS as usize * sample_rate as usize * channels as usize) / 1000`. Allocate / clear the ring with that capacity. Store both the buffer and capacity.
5. Modify the cpal callback (all three sample-format arms in `audio.rs`):
   - Always extend the pre-roll ring with the incoming `data` (after f32 conversion for I16/U16).
   - After extending, if `ring.len() > capacity`, drain the front: `ring.drain(0..(ring.len() - capacity))`.
   - This is done once per callback regardless of `is_recording`.
   - The existing `if rec.load(...)` block still appends to `samples` and updates the level. Keep that as-is.
6. Modify `AudioCapture::start()`:
   - After `is_recording.store(true, SeqCst)` and before returning, drain the ring into `samples`:
     - `let preroll: Vec<f32> = self.preroll.lock().drain(..).collect();`
     - `self.samples.lock().splice(0..0, preroll);` — prepend so the recording starts at the pre-roll point in time.
   - The streaming finalizer / feeder will pick up the prepended samples on their first poll, same as if the user had been speaking the whole time.
7. Modify `cancel()`:
   - Clear the pre-roll ring on cancel — we don't want a cancelled recording's audio leaking into a future press.
   - Actually, no: the ring is supposed to represent the last 300 ms regardless of recording state. Don't clear on cancel — clearing would defeat the pre-roll on a quick second press. Leave the ring alone.
8. On stream rebuild (device change in `start()`, or close-then-reopen): clear the ring and reallocate to the new device's sample-rate / channels. The watchdog idle-close path in TASK-36 also drops the stream — make sure it clears the ring too, so when the next stream opens, the ring is empty (not stuffed with stale samples from the old stream).
9. Add a tracing log: `[audio] start: prepended {} samples of pre-roll ({} ms)` so we can verify the path is firing.
10. Build: `cargo build --manifest-path src-tauri/Cargo.toml`. Resolve compile errors.
11. Run tests: `cargo test --manifest-path src-tauri/Cargo.toml`. Existing tests should still pass.
12. Add a small unit test if feasible: directly exercise the ring's truncation behavior — push 1000 samples into a ring with capacity 300, assert ring length stays at 300 and contains the latest 300. This doesn't require the stream.
13. Manual test on macOS:
    - Launch app, press PTT, hold for 2 s, release. Then immediately press PTT again and speak "hello world". The pre-roll should include the silence between the two presses; transcript should be "hello world" with no clip.
    - Press PTT, immediately say "test" and release as fast as you can. Transcript should include "test" cleanly. (Without pre-roll + warm-stream from TASK-36, fast-tap recordings often start mid-word.)
    - Run with `RUST_LOG=info`, look for the `prepended N samples of pre-roll` log line. Verify it shows ~300 ms worth of samples on warm-stream presses and 0 on the first cold press of a session.

## Success signal
- `cargo build --manifest-path src-tauri/Cargo.toml` exits 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; new ring-buffer truncation test passes.
- Manual test (macOS): a fast-tap recording where the user begins speaking before fully pressing the key produces a transcript with the leading word intact. A pre-fix baseline of the same fast-tap clips the leading word.
- `RUST_LOG=info` shows `[audio] start: prepended N samples of pre-roll` on warm-stream presses, with N matching ~300 ms × native rate × channels (e.g. 14400 for 48 kHz mono, 28800 for 48 kHz stereo).
- Cold first-press of a session shows `prepended 0 samples` — confirming the empty-ring path is correct.

## Notes
- The cpal callback runs on CoreAudio's high-priority thread. The TASK-22 callback-discipline comment at `audio.rs:339–355` says "no DSP, no channel sends, no heap-alloc beyond what `Vec::extend_from_slice` does." Mutex-locked `VecDeque::extend` + `drain` is a single short critical section — acceptable. If profiling shows lock contention, swap for a lock-free SPSC ring (e.g. `rtrb` crate) later. Don't pre-optimize.
- `VecDeque::drain(..)` allocates the iterator's collected `Vec`. If that becomes a problem, do `Vec::with_capacity(ring.len())` and `ring.drain(..).collect_into()`. Trivial.
- The `samples.splice(0..0, preroll)` at the start of recording is a one-time O(n) shift, but `samples` is empty at that point (just cleared in start()), so `splice` is effectively a `Vec::extend`. No worry.
- Memory: at 48 kHz stereo f32, 300 ms = 28,800 × 4 = 115 KB. Always-allocated. Fine.
- Privacy: the warm stream's audio passes through the ring even when not recording. It is held in RAM only, never written to disk, never sent over a channel. Same trust boundary as the warm stream itself (TASK-36).
