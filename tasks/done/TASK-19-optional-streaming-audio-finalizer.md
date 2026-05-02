# TASK-19: Optional streaming audio finalizer for long recordings

## Goal
Only if timing evidence shows post-release audio finalization is a real bottleneck, move toward a streaming finalizer that avoids resampling long stretches of dead air after release.

## Context
The current quality-first order is correct:

`capture native → downmix → resample 16 kHz → VAD trim → normalize → write WAV`

It does resample the full held recording, including silence. For normal push-to-talk dictation this is probably cheap compared with Whisper. For long recordings with lots of dead air, it may become noticeable.

This task is intentionally last because it adds complexity and should only happen after lifecycle, stage timing, VAD reuse, and persistent Whisper work.

## In scope
- Use TASK-13/TASK-15 timing evidence to decide whether this is needed.
- If needed, prototype a streaming finalizer:
  - capture native samples as today.
  - convert to 16 kHz mono incrementally off the audio callback.
  - run VAD over converted frames with prefill/hangover.
  - retain only the speech window needed for final WAV.
- Preserve final disk format: 16 kHz mono 16-bit PCM WAV.

## Out of scope
- Moving heavy DSP into the `cpal` callback.
- VAD-only auto-recording.
- Changing push-to-talk semantics.
- Compression codecs.
- Replacing Silero.

## Steps
1. Review timing logs after TASK-18. If Whisper still dominates and audio finalization is small, do not implement this task. Record that decision in `SESSION-STATUS.md`.
2. If implementation is justified, design a worker boundary:
   - audio callback remains append-only and level-only.
   - worker receives chunks or snapshots outside the callback.
   - worker owns resampler/VAD state.
3. Preserve the exact final output contract from TASK-13.
4. Add tests for:
   - output length and sample rate.
   - prefill/hangover retaining word boundaries on real or fixture audio if available.
   - no work inside callback beyond append/level update.
5. Run:
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
6. Manual proof:
   - short dictation still works.
   - long recording with leading/trailing silence finalizes faster than baseline.
   - no clipped first/last words.

## Success signal
- Either this task is explicitly deferred with evidence, or streaming finalization reduces long-recording finalization time without quality loss.
- No heavy work enters the audio callback.
- Final WAV format remains unchanged.

## Notes
- This is not the time to optimize unless measurements justify it.
- If implemented, do it as a careful pipeline change, not a quick pre-resample silence gate.

