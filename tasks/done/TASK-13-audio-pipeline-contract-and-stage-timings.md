# TASK-13: Codify the audio pipeline contract and add stage timing evidence

## Goal
TurboTalk has an explicit, testable audio pipeline contract:

`native mic capture → downmix mono → resample 16 kHz → Silero VAD trim → min-duration reject → peak normalize → write 16 kHz mono 16-bit PCM WAV`

The app also logs timing for each post-release stage so later optimization work is based on evidence instead of vibes.

## Context
The current audio implementation in `src-tauri/src/audio.rs` already follows the right quality-first order:

- Capture at the input device's native format.
- Do almost no work in the `cpal` callback.
- After release, downmix/resample once.
- Run Silero VAD after conversion because Silero expects 16 kHz mono f32.
- Normalize only the trimmed buffer.
- Write a temporary 16 kHz mono 16-bit PCM WAV for `whisper-cli`.

This task does not change the core order. It makes the order harder to accidentally regress and produces timing logs needed for later tasks.

## In scope
- `src-tauri/src/audio.rs`
  - Add named constants for target sample rate, target channels, target bits per sample, and minimum duration.
  - Add stage timing around downmix, resample, VAD, normalization, and WAV write.
  - Keep the `cpal` callback fast; no new processing inside the audio callback.
- Unit tests that assert the WAV spec and target-rate assumptions where practical.
- Small updates to `ARCHITECTURE.md` documenting the pipeline contract.

## Out of scope
- Changing the audio order.
- Streaming VAD.
- Changing Whisper flags.
- Persistent Whisper or VAD caching.
- Any frontend redesign.

## Steps
1. Read `src-tauri/src/audio.rs`, especially `stop()`, `downmix_to_mono`, `resample_to_16k`, `peak_normalize`, and the WAV writer block.
2. Add constants near the existing `NORMALIZE_PEAK`:
   - `TARGET_SAMPLE_RATE: u32 = 16_000`
   - `TARGET_CHANNELS: u16 = 1`
   - `TARGET_BITS_PER_SAMPLE: u16 = 16`
   - `MIN_RECORDING_MS: u32 = 100`
3. Replace hardcoded `16_000`, `1`, `16`, and `sample_rate / 10` style assumptions in `stop()` with the constants where it improves clarity.
4. Add lightweight timing using `std::time::Instant` inside `stop()`:
   - capture clone / stream drop
   - downmix
   - resample
   - VAD trim
   - normalize
   - WAV write
   - total finalization time
5. Emit one compact `tracing::info!` line with all stage timings after the WAV is written or discarded.
6. Keep log output readable. Avoid logging full buffers or excessive per-frame detail.
7. Add or update tests in `audio.rs`:
   - existing resample test still proves 48 kHz stereo → 16 kHz mono length.
   - add a small WAV-spec helper test if the spec construction is extracted.
8. Update `ARCHITECTURE.md` with a short "Audio Pipeline Contract" section that states:
   - disk handoff format is 16 kHz mono 16-bit PCM WAV.
   - compression codecs are intentionally not used in the Whisper handoff path.
   - silence trimming happens after resample because Silero requires 16 kHz mono f32.
9. Run:
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
10. Manually test one short recording and capture the timing log.

## Success signal
- Tests and clippy pass.
- A normal dictation still produces a transcript and paste.
- Runtime logs include a single clear finalization timing line for each accepted recording.
- `ARCHITECTURE.md` states the exact disk handoff format and the reason silence trimming happens after resampling.

## Notes
- Do not introduce MP3/AAC/Opus/FLAC. The temporary WAV is already small: 16 kHz × mono × 16-bit = 256 kbps, about 32 KB/s.
- This task intentionally produces evidence for TASK-17 and TASK-18.

