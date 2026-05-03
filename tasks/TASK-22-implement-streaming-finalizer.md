# TASK-22: Implement the streaming audio finalizer

## Goal
Drive long-recording finalization (`downmix + resample + vad + normalize +
wav_write`) from the measured **741.11 ms** down to **under 250 ms** on the
TASK-21 host (arm64 / macOS 26.4.1) — and ideally under 100 ms if the
streaming worker eliminates the resample-during-silence cost cleanly —
without quality regression. The audio callback must remain append-only and
level-only; the final on-disk WAV format (16 kHz mono 16-bit PCM) is
unchanged.

## Context
TASK-21 collected runtime stage timings under realistic recording on
arm64 / macOS 26.4.1 (build 25E253):

| | finalization sum | whisper | ratio |
|---|---|---|---|
| Short (~3.5s captured, 2.34s after VAD) | 150.96 ms | 3095 ms | 4.9% |
| Long (~27s captured, 22.89s after VAD)  | 741.11 ms | 2074 ms | 35.7% |

The long-recording ratio of **35.7%** clears the 30% gate; the absolute
**741.11 ms** clears the 250 ms gate. Both phase-3 conditions of TASK-21
pass, so the streaming finalizer is justified.

The dominator is `resample = 654.42 ms` — 88% of the long-recording
finalization sum. This is `rubato::FftFixedIn` running over the *entire*
held buffer, including the ~5s of leading + trailing silence VAD
subsequently trims. VAD itself trims ~20% of the buffer on long-with-
silence recordings (1323520 samples captured → 366240 written, of which
1.32M / 48k ≈ 27.6s captured, 22.89s kept). So we are spending ~130 ms
resampling audio we then immediately discard.

Prerequisite tasks already in place:
- **TASK-13** — pipeline contract + stage-timing instrumentation
- **TASK-14** — one-in-flight job lifecycle
- **TASK-15** — split transcribe / cleanup / paste stages
- **TASK-17** — cached Silero VAD session (avoids per-recording init)
- **TASK-20** — serialized `TranscriptionWorker` lifecycle wrapper

The cpal callback is already disciplined (append-only). The work to do is
moving the resample + VAD pipeline off the post-release critical path so
that by the time the user releases the hotkey, most of the silence has
already been resampled (or skipped) incrementally.

## In scope
- `src-tauri/src/audio.rs` — define the streaming-worker boundary;
  callback stays append-only and level-only as today.
- `src-tauri/src/audio_finalizer.rs` *(new module, optional)* — owns
  resampler + VAD state and processes chunks off the callback thread.
- Worker channel plumbing — chunk handoff from capture thread to
  finalizer worker; backpressure handling; clean shutdown on stop.
- Tests for: output length, sample rate, worker-side prefill/hangover
  preserving word boundaries on a fixture recording, no work added to
  the audio callback.

## Out of scope
- Moving DSP into the cpal callback (callback stays minimal).
- VAD-only auto-recording (push-to-talk semantics unchanged).
- Push-to-talk hotkey behavior changes.
- Compression codecs.
- Replacing Silero VAD.
- Audio file format changes — final WAV stays 16 kHz mono 16-bit PCM.
- Whisper-side changes (TASK-20 territory).

## Steps
1. **Design the worker boundary.** Document in a code comment at the top
   of `audio.rs` (or the new `audio_finalizer.rs`) the contract:
   - capture thread: appends raw native samples + updates level meter; no
     other work.
   - finalizer worker: owns `rubato` resampler state, VAD session
     reference, and a rolling buffer of resampled-but-not-yet-decided
     samples.
   - chunk size: pick a power-of-two frame count that aligns with both
     cpal callback periods and the VAD frame size (Silero v4 expects
     512-sample frames at 16 kHz).
2. **Implement incremental resample off the callback.** Send native
   chunks from the capture thread to the worker via a bounded channel.
   Worker calls `rubato::process` per chunk and accumulates 16 kHz
   samples. Preserve resampler state across chunks (do not re-init per
   chunk).
3. **Implement incremental VAD with prefill/hangover.** Run Silero on
   each new 16 kHz frame as it arrives. Maintain a hangover window so
   word endings aren't clipped, and a small prefill ring buffer so word
   onsets aren't clipped either. This is the standard VAD streaming
   pattern — same as what `cjpais/Handy` does.
4. **Preserve the final WAV output contract.** On stop, the worker
   flushes remaining frames, applies peak normalization to the kept
   speech window, and writes 16 kHz mono 16-bit PCM via `hound` —
   exactly matching today's bit-for-bit output for non-silence inputs.
5. **Update `[audio] stage timings` log.** Add per-phase timings for
   the streaming path: e.g. `incremental_resample_total`,
   `incremental_vad_total`, `finalize_flush`. Keep the legacy
   `total=` field for direct comparison against TASK-21 evidence.
6. **Tests:**
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - Unit test: same input buffer through old and new path produces the
     same kept-sample count (within VAD framing tolerance) and same
     peak-normalized output.
   - Property test or fixture test: prefill/hangover retains word
     boundaries on a known recording with leading and trailing silence.
   - Smoke test: no allocations or DSP calls inside the cpal callback
     (verify by code inspection + a counter-style guard).
7. **Build hygiene:**
   - `cargo build --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
8. **Manual proof:** dictate a short phrase and a long recording with
   leading + trailing silence on the same host (arm64 / macOS 26.4.1).
   Capture fresh `[audio] stage timings` and `[transcribe] whisper took`
   lines for both. Compare against TASK-21's archived evidence.

## Success signal
- Long-recording finalization measured under **250 ms** on the same host
  as TASK-21 (arm64 / macOS 26.4.1).
- Short-recording finalization unchanged or better — definitely not
  worse.
- No clipped first or last words on the long-with-silence recording when
  played back or transcribed.
- Audio cpal callback still does only append + level-meter update;
  verifiable by code inspection.
- `cargo build`, `cargo test`, `cargo clippy -D warnings` all clean for
  `src-tauri`.

## Notes
- **Re-run timing protocol after landing.** Once this lands, re-run the
  recording recipe documented in `tasks/done/timing-protocol.md` and
  paste the new `[audio] stage timings` + `[transcribe] whisper took`
  lines into a fresh evidence section in this task file (or its archive
  copy). Compare directly to TASK-21's 741.11 ms baseline.
- **Reference** `tasks/done/timing-protocol.md` for the recording recipe
  (short and long-with-silence scenarios, exact phrasing, what to copy).
- **Warn against pre-resample silence gates.** The cheap-looking
  optimization — gating raw native samples through a fast RMS or VAD
  step *before* resampling — risks clipping word onsets and offsets,
  which is exactly the regression TASK-11 was designed to prevent. Do
  the streaming work the proper way: incremental resample + Silero VAD
  with prefill/hangover, same Silero model and frame size as today.
- The cpal callback discipline is non-negotiable. If a chunk handoff
  introduces backpressure, drop chunks at the worker side, never at the
  callback side. Capture path must never block.
- Reference impls: `cjpais/Handy` (production-quality streaming VAD in
  Rust + Tauri); `whisper.cpp` `stream` example (incremental VAD frame
  layout).
