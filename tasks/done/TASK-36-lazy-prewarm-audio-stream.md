# TASK-36: Lazy pre-warm of audio stream with idle timeout

## Goal
Pressing the PTT hotkey starts capturing samples within ~10 ms instead of waiting for CoreAudio stream startup. After recording stops, the stream stays open for an idle window (default 45 s) and then closes; back-to-back recordings within that window are instant.

## Context
The user reports a ~500 ms delay between hotkey press and audio capture, severe enough that the first word(s) of dictation get clipped. Investigation showed the path in `src-tauri/src/audio.rs::AudioCapture::start()` is:

1. `settings::load()` — config file I/O
2. `open_stream()` — `device.default_input_config()`, `build_input_stream()`, `stream.play()` — CoreAudio device init
3. `StreamingFinalizer::start()` — spawns worker thread, inits rubato resampler
4. `spawn_feeder()` — spawns feeder thread
5. `is_recording.store(true)`

CoreAudio takes ~50–200 ms (built-in mic) or 200–500 ms (Bluetooth) after `stream.play()` before the first input callback fires, even though `is_recording=true` is set immediately. That's the dominant source of the delay.

The fix: lazy pre-warm. Keep the cpal stream open between recordings, with a per-press `is_recording` flag controlling whether samples are kept. Close the stream after an idle timeout so the macOS mic indicator only shows during active dictation sessions, not the entire app lifetime.

Reference: this is the model `cjpais/Handy` uses (warm stream, flag-gated capture).

Key constraint: the macOS mic indicator (orange dot in menu bar / Control Center) is visible the entire time the cpal stream is open. The idle timeout is what keeps it from being on 24/7. 45 s is a reasonable default — long enough to catch back-to-back dictation, short enough that the user sees the indicator clear when they walk away.

The current state machine in `src-tauri/src/recorder.rs` (`Ready → Recording → FinalizingAudio → Transcribing → Cleaning → Pasting → Ready`) does not need to change — only the underlying `AudioCapture` does. From the recorder's perspective `start()` and `stop()` keep the same contract.

The streaming finalizer worker (`StreamingFinalizer` in `src-tauri/src/audio_finalizer.rs`) is per-recording — it owns the resampler/VAD state for one job. It must still be spawned per-recording inside `start()` and joined in `stop()`. Do not try to keep it warm across recordings.

The capture-feeder thread (in `audio.rs::spawn_feeder`) is also per-recording for the same reason. Must still be spawned per-recording.

So what stays warm: the cpal `Stream` itself, the device/config/sample-rate metadata. What stays per-recording: `samples` buffer (cleared on each start), `StreamingFinalizer`, `feeder` thread, `is_recording` flag.

Device-change handling: today the stream is rebuilt on every press, so a built-in mic ↔ AirPods switch is picked up automatically. With pre-warm, the stream is built once and cached. We need a way to re-open if the configured device changes. Two reasonable options:
- (A) Re-read the configured device at every `start()`; if it differs from the warm stream's device, close and re-open.
- (B) Subscribe to device changes via cpal/CoreAudio.

Pick (A) — simple, correct, and only adds a nanosecond mutex read (since TASK-38 will cache the config). On a device change, the user pays the cold-start latency once for that press; that's acceptable.

The level-broadcast thread in `src-tauri/src/lib.rs:659` (50 ms loop calling `recorder.is_recording()` and `recorder.device_lost()`) does not need changes.

## In scope
- `src-tauri/src/audio.rs` — `AudioCapture` struct and lifecycle
- A new idle-timeout watchdog (likely a thread or a `tokio::time::sleep` if we want async, but a plain thread is fine)
- Tests in `src-tauri/src/audio.rs` if any, plus a new test for the warm-stream path if it can be exercised without a real device (probably can't — that's OK, document why)

## Out of scope
- `src-tauri/src/recorder.rs` — its public API and state machine stay the same
- `src-tauri/src/audio_finalizer.rs` — `StreamingFinalizer` lifecycle stays per-recording
- `src-tauri/src/hotkey.rs` — call site for `recorder.start()` / `recorder.stop()` stays the same
- The pre-roll ring buffer (TASK-37) — separate task, builds on this
- Config file-I/O caching (TASK-38) — separate task, independent
- Frontend / overlay changes — none needed
- Adding a user-facing setting for the idle timeout — keep it as a hardcoded `const IDLE_TIMEOUT: Duration = Duration::from_secs(45);` for now

## Steps
1. Read `src-tauri/src/audio.rs` end-to-end to internalize the current lifecycle. Pay special attention to the cpal callback's check of `is_recording` (it already discards samples when the flag is false — that property is what makes pre-warm safe).
2. Add new fields to `AudioCapture`:
   - `warm_stream: Mutex<Option<ActiveStream>>` (replaces today's `active`, conceptually) — holds the cpal stream when warm or recording.
   - `warm_device_name: Mutex<Option<String>>` — the device the warm stream was opened against, for the device-change check.
   - `idle_close_at: Arc<Mutex<Option<Instant>>>` — set when `stop()` returns; the watchdog reads this and closes the stream once `Instant::now() > idle_close_at`. `None` means "do not close".
   - `watchdog_handle: Mutex<Option<JoinHandle<()>>>` — the spawned watchdog thread.
   - `shutdown_watchdog: Arc<AtomicBool>` — set on `Drop` (or a new `shutdown()` method) to tell the watchdog to exit.
3. Spawn the watchdog thread in `AudioCapture::new()`. The watchdog loops forever:
   - Sleep ~1 s.
   - If `shutdown_watchdog` is set, exit.
   - Read `idle_close_at`. If `Some(deadline)` and `Instant::now() >= deadline`:
     - Take the `warm_stream` lock; if a stream is present AND `is_recording` is false, drop the stream and clear `warm_device_name`. (The is_recording check guards against a race where `start()` flipped the flag between the watchdog reading `idle_close_at` and acquiring the lock.)
     - Set `idle_close_at` to `None`.
4. Refactor `AudioCapture::start()`:
   - Read `samples.lock().clear()`, `level.store(0)`, `device_lost.store(false)`, `feeder_stop.store(false)`, `feeder_cursor.store(0)` as today.
   - Read configured device from settings (will become a cached read after TASK-38).
   - If `warm_stream` is `Some` AND `warm_device_name` matches the configured device: skip stream init.
   - Otherwise: drop any existing warm stream, call `open_stream(want)`, store result in `warm_stream`, store name in `warm_device_name`.
   - Cancel the idle close: set `idle_close_at` to `None`.
   - Spawn `StreamingFinalizer` and the capture-feeder as today.
   - `is_recording.store(true)` — samples now start being retained.
5. Refactor `AudioCapture::stop()`:
   - `is_recording.store(false)` — same as today.
   - Sleep 25 ms — let the in-flight callback finish.
   - **Do not** drop the stream; leave it in `warm_stream` so the next `start()` is instant.
   - Read sample-rate / channels from `warm_stream` (was previously `active`).
   - Drain feeder, finish StreamingFinalizer, build WAV — all as today.
   - Set `idle_close_at = Some(Instant::now() + IDLE_TIMEOUT)` so the watchdog will close the stream after the idle window.
6. Refactor `AudioCapture::cancel()`:
   - Same cleanup of feeder + finalizer + samples + level as today.
   - **Do not** drop the warm stream — leave it warm for the next press.
   - Set `idle_close_at = Some(Instant::now() + IDLE_TIMEOUT)`.
7. Implement `Drop` for `AudioCapture`:
   - Set `shutdown_watchdog` to true.
   - Join the watchdog thread.
   - Drop the warm stream (implicit on field drop).
8. Verify `device_lost` handling still works:
   - The error callback sets `device_lost=true` and `is_recording=false`.
   - The level-broadcast thread observes `device_lost` and calls `recorder.cancel()`.
   - `cancel()` (above) leaves the warm stream — but `device_lost` means the stream is broken. Add: in `cancel()`, if `device_lost` was edge-set this call, drop the warm stream too. Use `device_lost: AtomicBool` directly here (don't go through the `device_lost()` swap-on-read accessor — the level thread already swapped it).
   - Actually simpler: any time `device_lost` is true at the start of `start()` or `cancel()`, drop the warm stream. Pick whichever is cleanest.
9. Logging: add `tracing::info!` lines for "stream pre-warmed", "stream reused (warm)", "stream closed (idle timeout)", "stream closed (device change)". Keep the existing `[audio] recording started` log too.
10. Build: `npm run tauri dev` (or `cargo build --manifest-path src-tauri/Cargo.toml`). Resolve any compile errors.
11. Run cargo tests: `cargo test --manifest-path src-tauri/Cargo.toml`. The existing audio.rs tests should still pass (resampler / downmix / normalize / spec). Recorder lifecycle tests should still pass.
12. Manual test on macOS:
    - Launch app, press PTT, speak immediately ("hello world"), release. Verify "hello world" pastes and the leading audio is not clipped (compare against a pre-fix baseline — the user says first words currently get cut).
    - Wait 60 s. Check `~/Library/Logs/...` or run with `RUST_LOG=info` in a terminal and look for `[audio] stream closed (idle timeout)`. Verify the macOS mic indicator (orange dot) clears.
    - Press PTT again — should be instant.
    - Change input device in System Settings → Sound, then press PTT — should pick up the new device and re-open.

## Success signal
- `cargo build --manifest-path src-tauri/Cargo.toml` exits 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` exits 0.
- Manual test (macOS): pressing PTT and speaking "hello world" with no perceptible delay produces a transcript starting with "hello world" — the leading word is not clipped. With the previous behavior, the leading word was clipped.
- After 45–60 s of idle, the macOS mic indicator clears (orange dot disappears from the menu bar).
- A second PTT press within the idle window starts capturing instantly (no perceptible delay).
- `RUST_LOG=info` shows the new tracing lines: `stream pre-warmed`, `stream reused (warm)`, `stream closed (idle timeout)`.

## Notes
- The cpal `Stream` is `!Send` on macOS by default; `AudioCapture` already has `unsafe impl Send for AudioCapture` and `unsafe impl Send for ActiveStream`. The safety comment at `audio.rs:99–141` covers why this is sound. The pre-warm change does not violate it — the stream is still only touched from one thread (the hotkey worker thread) via the `warm_stream` mutex. Update the comment if the field name changes.
- Watch out for double-close: if the watchdog fires while a `start()` is in flight, both could race on `warm_stream`. The `is_recording` check inside the watchdog handles the post-start race; the `warm_stream` mutex serializes the close vs. the new-stream-replace.
- Don't forget to clear `samples` in `start()` (it's already there). The warm stream's callback would happily keep appending if you forgot.
- Pre-roll ring buffer (TASK-37) will hook into the cpal callback — that task adds capture even when `is_recording=false`. Keep that contract in mind: today the callback discards on `!is_recording`, but TASK-37 will change that. Don't entrench the discard behavior in a way that's hard to undo.
