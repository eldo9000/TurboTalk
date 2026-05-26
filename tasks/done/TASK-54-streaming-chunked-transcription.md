# TASK-54: Streaming chunked transcription (unbounded recording length)

## Goal

Transcribe long dictations as a sequence of segments cut on silence boundaries
*during* recording, instead of one whole-file POST after release. The final
paste becomes near-instant regardless of recording length, and no single
whisper-server request ever approaches the request-timeout ceiling.

## Why (root cause that motivated this)

A ~3 min recording (job 251, 4,206,240 samples @ 24 kHz = 175 s of dense
speech) failed with `"error sending request"` at **exactly 30.047 s** after
capture. Investigation (2026-05-19):

- whisper-server is **not** the cause. Live-server tests against the running
  PID: 180 s WAV → HTTP 200 in 23.8 s; 480 s WAV (15.3 MB) → HTTP 200 in
  55.2 s. It handles long audio and large payloads fine.
- Real cause: reqwest's **blocking** client defaults to a 30 s total request
  timeout (`Timeout::default() = Some(Duration::from_secs(30))`,
  reqwest-0.12.28 `src/blocking/client.rs:1503` — the *async* client defaults
  to `None`). `transcribe.rs` built the client with `Client::new()` and
  silently inherited it. Dense 175 s speech transcribes in >30 s, so the
  client cut the connection mid-inference; the server kept running (same PID
  survived), which is why it surfaced as a transport error, not an HTTP 500.

**Already shipped (safety net, separate from this task):** the transcribe
client now sets an explicit 120 s timeout (`transcribe.rs`, ~line 392). That
alone stops the silent 30 s death on the whole-file path. This task is the
UX upgrade on top of it: amortize whisper time across the recording so the
user never waits 40–60 s after releasing a long dictation, and keep every
request structurally far below any timeout.

## Context — the seam already exists

`audio_finalizer.rs` already does almost everything needed, *during* recording,
off the cpal callback thread:

- The finalizer worker (`run_worker`) incrementally resamples native audio to
  16 kHz mono into `resampled_buf` and runs streaming Silero VAD frame by
  frame via `run_vad_on_new_frames`.
- `StreamingVad::observe` (`audio_finalizer.rs:210`) already tracks the
  `in_speech` state machine with onset (2 frames) and hangover (15 frames)
  smoothing. **A silence boundary is the `(true → false)` transition once
  hangover is exhausted** — exactly the clean cut point we want. No new DSP.
- `TranscriptionWorker` (`transcribe.rs`) serializes all requests through
  `spawn_lock`, so background segment POSTs and the final-tail POST queue
  against the one warm server with no race (honours the TASK-14 one-in-flight
  invariant).
- The canonical `samples: Arc<Mutex<Vec<f32>>>` buffer in `AudioCapture` is
  retained for the whole recording, so a full batch-fallback POST is always
  possible if the streaming-transcribe path degrades — same philosophy as the
  existing TASK-22 streaming finalizer / batch fallback split.

## Design

### Segmentation (in `audio_finalizer.rs`)
- As VAD runs, track the current segment's start frame. Emit a **segment cut**
  when BOTH hold:
  - speech has just transitioned to silence (hangover exhausted), AND
  - the segment is at least `MIN_SEGMENT_SECS` long (≈ 12 s) of accumulated
    16 kHz audio.
- Hard ceiling: force a cut at `MAX_SEGMENT_SECS` (≈ 25 s) even without a
  silence boundary, so request time and memory stay bounded. A forced mid-word
  cut is rare and tolerable; prefer silence cuts.
- Each emitted segment is the peak-normalized 16 kHz mono slice for that
  window (reuse the existing trim/normalize logic, applied per segment instead
  of once at Finish). Clean silence cuts mean no word is split → keep
  `no_context=true` (already set) with no overlap/stitching.

### Transcription dispatch (new, small module or extend `transcribe.rs`)
- A bounded queue receives `(segment_index, Vec<f32>)`. A background worker
  writes each to a temp WAV and calls the existing `TranscriptionWorker`
  (warm server, serialized). Results land in an ordered map keyed by
  `segment_index`.
- On `stop()`/Finish: the finalizer emits the final un-cut tail as the last
  segment. The orchestrator waits for all in-flight segment transcriptions,
  assembles texts in `segment_index` order, joins with single spaces.

### Cleanup + paste (in `hotkey.rs` / `recorder.rs`)
- Run `cleanup::process` **once** on the joined raw transcript at finalize.
  Chaperone's classify is a single bounded LLM call regardless of length, so
  full-text cleanup preserves current behavior and full context. The expensive
  part (whisper) is what got amortized; cleanup stays end-of-pipeline.
- Paste the cleaned result, unchanged from today.

### Failure handling (must not lose audio)
- Per-segment POST: retry once; on repeated failure, abandon the streaming
  result and fall back to the whole-file batch POST against the canonical
  `samples` buffer (now with the 120 s timeout). No recording is ever lost.
- If the finalizer/VAD degrades (model load failure, resampler init failure),
  fall straight through to today's batch path — same as TASK-22.

## In scope
- `src-tauri/src/audio_finalizer.rs` — segment-boundary detection + per-segment
  emit (new message variant on the worker → owner channel).
- `src-tauri/src/transcribe.rs` — ordered segment transcription queue reusing
  `TranscriptionWorker`; explicit timeout (already done).
- `src-tauri/src/audio.rs` / `recorder.rs` / `hotkey.rs` — collect ordered
  segment texts; on stop, assemble + transcribe final tail + cleanup + paste;
  batch fallback wiring.
- Tests: segmentation boundary unit tests (synthetic `is_voice` sequences,
  same style as the existing `streaming_smoothing_*` tests); ordered-assembly
  test; fallback test.

## Out of scope
- Live partial-result display in the overlay (could come later; not needed for
  the latency win).
- Per-segment cleanup / streaming Chaperone.
- Changing the whisper decode flags.
- The two overlay UX features (elapsed-time ticker, flash-to-warn) — file as
  separate small tasks; they layer on independently and partly mitigate this
  until it lands.

## Proof (Tier 1 — name it before calling it done)
- Record ≥ 3 min of continuous real speech; full transcript pastes correctly,
  in order, with no dropped or duplicated words at segment seams.
- Finalize-to-paste latency after release is roughly the time to transcribe
  only the final tail segment (a few seconds), not the whole recording.
- Kill Ollama mid-recording (or force a segment POST failure) → batch fallback
  produces the complete transcript; nothing lost.
- `cargo test`, `cargo clippy -D warnings`, `npm run build` green.

## Risk / split note
This touches the finalizer state machine, a new transcription queue, and the
hotkey orchestration — likely more than one focused session. If it feels like
more than a single session, **split before continuing** (suggested split:
(A) segmentation emit in the finalizer with a unit test; (B) ordered
transcription queue + assembly; (C) hotkey wiring + fallback). Do not push
through as one mega-change.
