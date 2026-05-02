# TASK-9: Resample mic input to 16 kHz mono before WAV write

## Goal
The WAV file passed to whisper.cpp is **16 kHz, mono, 16-bit PCM** regardless of the device's native rate or channel count. `cpal` stays at the device's native rate (so AirPods / Bluetooth / aggregate devices keep working); resampling happens once per recording in `audio.rs::stop()` using `rubato::FftFixedIn`. The existing `trim_silence` and `min_samples` logic operates on the resampled mono buffer.

## Context
TurboTalk is a personal-use macOS dictation app. Today, `src-tauri/src/audio.rs` writes the WAV at whatever rate and channel count `cpal` chose for the input device — typically 44.1 kHz or 48 kHz, often stereo on AirPods. Whisper's front-end expects 16 kHz mono and resamples internally, but doing the conversion ourselves with proper anti-aliasing measurably improves output quality and removes edge cases on Bluetooth devices.

Reference implementation: cjpais/Handy (open-source Tauri+Rust dictation app, MIT) does exactly this. Their resampler logic lives at `/tmp/handy-ref/src-tauri/src/audio_toolkit/audio/resampler.rs` and the call site is `recorder.rs:282-335`.

Current relevant code in `src-tauri/src/audio.rs`:
- `stop()` reads the accumulated buffer, calls `trim_silence(&buf, sample_rate)`, then writes a `hound::WavSpec { channels, sample_rate, bits_per_sample: 16, sample_format: Int }`.
- The cpal callback (lines ~180–225) accumulates samples as `Vec<f32>` regardless of the source format (i16/u16 are converted to f32 in the callback).
- `sample_rate` and `channels` come from `device.default_input_config()` — these are the device's native values.

## In scope
- `src-tauri/Cargo.toml` — add `rubato` dependency (latest 0.15.x or 0.16.x).
- `src-tauri/src/audio.rs` — downmix multi-channel buffers to mono; resample to 16 kHz; update the WAV spec; update `trim_silence` and `min_samples` to use the new 16 kHz rate.

## Out of scope
- Don't change `cpal`'s stream config — it stays at the device default. Forcing cpal to 16 kHz breaks Bluetooth and aggregate devices.
- Peak normalization (covered by TASK-10).
- VAD (covered by TASK-11).
- whisper-cli flag tuning (covered by TASK-12).
- Streaming / real-time resampling — buffer-once-at-end is fine; the recording is already finished by the time `stop()` runs.

## Steps
1. Read `src-tauri/src/audio.rs` end-to-end. Note the structure of `stop()` and how `sample_rate` / `channels` flow into `trim_silence` and the `hound::WavSpec`.
2. Add `rubato = "0.15"` to `[dependencies]` in `src-tauri/Cargo.toml`.
3. In `audio.rs`, add a helper `fn downmix_to_mono(buf: &[f32], channels: u16) -> Vec<f32>`:
   - If `channels == 1`, return `buf.to_vec()`.
   - Else, average each frame: `out[i] = (sum of buf[i*channels .. (i+1)*channels]) / channels as f32`.
4. Add a helper `fn resample_to_16k(buf: &[f32], src_rate: u32) -> anyhow::Result<Vec<f32>>`:
   - If `src_rate == 16_000`, return `buf.to_vec()`.
   - Else, build a `rubato::FftFixedIn::<f32>::new(src_rate as usize, 16_000, /* chunk_size */ 1024, /* sub_chunks */ 2, /* nbr_channels */ 1)` and process the input in chunks. The crate returns `Vec<Vec<f32>>` (one Vec per channel); we only have one channel so return `out[0]` of the concatenated chunks. Pad the final chunk with zeros if needed; truncate trailing zeros from the output (rubato adds latency padding).
   - Look at `/tmp/handy-ref/src-tauri/src/audio_toolkit/audio/resampler.rs` for the chunk-loop pattern.
5. In `stop()`, after the buffer is locked-and-cloned and BEFORE the existing `trim_silence` call:
   - `let buf = downmix_to_mono(&buf, channels);`
   - `let buf = resample_to_16k(&buf, sample_rate)?;`
   - Override the local `sample_rate` to `16_000` and `channels` to `1` for the rest of the function.
6. The existing `trim_silence` and `min_samples` calculations now use the 16 kHz rate automatically because they read the local `sample_rate`. Verify the chunk math (`sample_rate / 50` for 20 ms windows) still produces an integer chunk size.
7. The `hound::WavSpec` is now hardcoded `channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: Int`.
8. Run `cargo build --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`. Both must exit 0.
9. Run `cargo test --manifest-path src-tauri/Cargo.toml`. All existing tests must pass.
10. Add a unit test in `audio.rs` that feeds a synthetic stereo 48 kHz buffer through `downmix_to_mono` then `resample_to_16k` and asserts the output length is approximately `input_frames * 16000 / 48000` (within ±32 samples for resampler latency).
11. Manually test:
    - Run the app, record "hello world", pull the latest temp `.wav` (look for it via `tracing` logs — `audio.rs::stop()` logs the path).
    - `afinfo /tmp/...wav` should show **1 channel · 16000 Hz · 16-bit**.
    - Confirm the transcript still says "hello world" (or close — accuracy may *improve*, not regress).

## Success signal
- WAV files written by `audio.rs::stop()` are 16 kHz mono 16-bit (verified with `afinfo`).
- `cargo build` and `cargo clippy -- -D warnings` exit 0.
- `cargo test` passes including the new resample unit test.
- "Hello world" smoke transcript is at least as accurate as before (no regression).

## Notes
- The cpal callback stays exactly as-is — all conversion happens after recording ends, in `stop()`. This keeps the audio thread fast.
- `rubato` returns `Vec<Vec<f32>>` indexed by channel. We feed one channel in (the downmixed mono) and read `out[0]` out.
- This task **must land before TASK-10 and TASK-11**, because both of those operate on the 16 kHz mono buffer this task creates.
- TASK-12 (whisper flag tuning) is independent and can run in parallel.
- Don't modify the temp-file lifecycle (RAII guard in `recorder.rs`); only the contents written into the file change.
