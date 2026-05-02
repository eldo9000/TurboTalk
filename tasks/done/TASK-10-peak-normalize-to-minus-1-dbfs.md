# TASK-10: Peak-normalize the recorded buffer to ~-1 dBFS before WAV write

## Goal
Before writing the WAV file, the recorded f32 buffer is peak-normalized so its maximum absolute sample equals **0.89 (≈ -1 dBFS)**. Quiet recordings from low-gain laptop mics no longer under-fill whisper's mel front-end or trigger transcription hallucinations. Loud recordings are not attenuated — normalization is one-way (boost only).

## Context
TurboTalk is a personal-use macOS dictation app. Users have reported that transcription quality is noticeably worse than competing apps. Multi-source research identified the lack of audio normalization as the most likely root cause: built-in MacBook microphones typically peak between -25 and -18 dBFS, well below what whisper.cpp was trained on. Documented evidence:
- [whisper hallucination on quiet audio (faster-whisper #183)](https://github.com/SYSTRAN/faster-whisper/issues/183)
- [Calm-Whisper paper on hallucination reduction via normalization](https://arxiv.org/html/2505.12969v1)

This is a single-pass scan + scalar multiply on the f32 buffer. No new dependencies, no DSP library.

Current relevant code in `src-tauri/src/audio.rs::stop()`:
- Locks the accumulated buffer.
- Calls `trim_silence(&buf, sample_rate)` → returns `(start, end)` indices or `None` if too quiet.
- Slices `let trimmed = &buf[start..end]` (or similar — the exact shape may differ).
- Iterates `trimmed`, converts each sample `(s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16`, and writes via `hound::WavWriter`.

The normalization step inserts itself between `trim_silence` and the WAV write loop.

## In scope
- `src-tauri/src/audio.rs` — add a `peak_normalize(samples: &mut [f32], target: f32)` helper, call it from `stop()` on the trimmed buffer before the WAV write loop.
- One unit test exercising boost-when-quiet and pass-through-when-loud.

## Out of scope
- RMS-based normalization, dynamic range compression, AGC, look-ahead limiters — peak only, one-way (boost only).
- Per-chunk normalization in the cpal audio callback — the callback stays fast; normalize once at the end.
- Any change outside `audio.rs`.
- DC-offset removal (not currently a problem; revisit if reported).
- Resampling (TASK-9) and VAD (TASK-11) — separate tasks.

## Dependencies
- This task **should land after TASK-9** (resample to 16 kHz mono). Reason: TASK-9 changes the buffer that's being normalized. Working on the post-resample buffer means we normalize the actual audio whisper sees. If TASK-9 isn't merged, the work still applies — just operates on the device-native buffer at the same insertion point.

## Steps
1. Read `src-tauri/src/audio.rs::stop()` end-to-end. Find the point after `trim_silence` returns and before the WAV write loop. Note the variable name holding the trimmed buffer (likely `trimmed: &[f32]` or similar).
2. If the trimmed buffer is currently a `&[f32]` slice borrowed from `buf`, change it to a `Vec<f32>` (or `Cow<[f32]>`) so it can be mutated in place. The existing flow probably already owns the data after slicing — if so, leave it as a `Vec`.
3. Add a private helper to `audio.rs`:
   ```rust
   /// Peak-normalize a buffer of f32 samples to the given target peak.
   /// One-way: boosts quiet input; never attenuates loud input.
   fn peak_normalize(samples: &mut [f32], target: f32) {
       let peak = samples.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
       if peak > 0.0 && peak < target {
           let gain = target / peak;
           for s in samples.iter_mut() {
               *s = (*s * gain).clamp(-1.0, 1.0);
           }
       }
   }
   ```
4. In `stop()`, after `trim_silence` returns the trimmed buffer and before the `WavWriter` loop:
   - `peak_normalize(&mut trimmed, 0.89);`
   - The `0.89` constant is ≈ -1 dBFS. Add a `const NORMALIZE_PEAK: f32 = 0.89;` near the top of the file with a one-line comment.
5. Add a `#[cfg(test)] mod tests` (or extend an existing one) with two tests:
   - `peak_normalize_boosts_quiet_buffer`: input peak 0.1, target 0.89, expect output peak in `[0.88, 0.90]` (allowing for f32 rounding).
   - `peak_normalize_leaves_loud_buffer_alone`: input peak 0.95, target 0.89, expect output peak == 0.95 (unchanged).
6. Run `cargo build --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, `cargo test --manifest-path src-tauri/Cargo.toml`. All three must exit 0.
7. Manually test:
    - Speak quietly into the built-in MacBook mic. Compare the saved WAV's audible loudness before-and-after the change (use `afplay`).
    - Verify a normal-volume recording (close to the mic) is unchanged — no clipping artifacts, no compression sound.
    - Verify the transcript is at least as accurate as before. The whole point is that quiet input transcribes *better* — listen for the "thanks for watching" / " you" / "um" hallucinations on near-silent recordings; they should be reduced or eliminated.

## Success signal
- For any recording where the original peak < 0.89, the WAV's peak amplitude lands in `[0.85, 0.95]`.
- For any recording where the original peak ≥ 0.89, the WAV is unchanged at the sample level.
- Both unit tests pass.
- Quiet-speech transcription accuracy is visibly improved on a real-world test.
- `cargo build`, `cargo clippy -- -D warnings`, `cargo test` exit 0.

## Notes
- Target 0.89 ≈ -1 dBFS — the headroom Handy and other reference dictation apps use. Don't push to 1.0 (clipping artifacts on the i16 conversion).
- The `clamp(-1.0, 1.0)` after the gain multiply is defensive; if the math is right it should never engage. The existing WAV write loop also clamps before the i16 cast, so there are two layers of safety.
- Don't subtract DC offset. Whisper handles DC offset internally and removing it can introduce artifacts on some mic types.
- This task is independent of TASK-11 (VAD) and TASK-12 (whisper flags) — they touch different stages of the pipeline.
