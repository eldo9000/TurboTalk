# TurboTalk — Session Status

**Last updated:** 2026-05-02
**Current state:** Dictation-quality sprint complete. 4/4 tasks landed.
Hardening sprint already closed (8/8). Audio pipeline now does explicit
16 kHz mono resampling, peak-normalization, Silero VAD, and tuned
whisper-cli flags — addresses the "mic not sensitive enough" complaint.

## Where We Are

Roadmap M0 and M1 are done. Core loop works:
Right Alt → record → whisper transcription → paste into focused app.

Commits this session:
- `5063e13` scaffold: Tauri 2 + Svelte 5 + librewin foundation
- `12726ba` feat: hotkey + mic capture (CGEventTap, cpal, hound)
- `0d5d5be` feat: whisper transcription (whisper-cli, ggml-base.en)
- `6557ed8` feat: paste into focused app (arboard + osascript)
- `0c00ece` fix: UI chrome + transcribing state reset

## Active Focus

Optimization planning for the dictation pipeline.

## Current Planning Output

Created ordered task files TASK-13 through TASK-19 in `tasks/`.
They cover audio/codec invariants, one-in-flight lifecycle, stage separation
with job ids, paste focus policy, VAD reuse, persistent Whisper, and optional
streaming finalization.

## Blockers

None.

## Next action

Dispatch `tasks/TASK-13-audio-pipeline-contract-and-stage-timings.md`;
success signal is a normal dictation plus timing logs for each audio
finalization stage.

## Hardening Sprint (2026-05-01) — closed

Multi-agent code review (security + architecture) → 8 tasks dispatched + landed.
All commits on main; tasks/done/ has the archived task files.

- `0b4d606` fix(security): CSP enabled in Tauri config (closes XSS class)
- `a8a75cd` fix(security): canonicalize subprocess paths (closes path traversal)
- `62243c7` docs(audio): SAFETY argument for unsafe Send/Sync on AudioCapture
- `d78abd4` refactor(cleanup): typed mode, URL allowlist, prompt isolation, 2s timeout
- `882ebdd` fix(recorder): type-enforce state transitions; paste-error/discarded events
- `4a0b654` fix(audio): RAII temp files, device-loss detection, recording-too-short
- `3526050` fix(history): backend-owned 50-entry limit, awaited save, ui-error channel
- `1a1ebed` chore(types): tauri-specta typed contract + multi-monitor overlay + SAFETY/TRUTH

Reports archived at `/tmp/static-analysis-main-20260501-1200.md` and
`/tmp/code-analysis-concern-based-main-20260501.md`.

## Dictation-quality sprint (2026-05-01) — closed

Research synthesized from cjpais/Handy reference impl, whisper.cpp issue
threads, and DSP literature → 4 tasks dispatched + landed via /triage-dispatch.
User-reported symptom: "mic not sensitive enough, output much worse than other
dictation apps." Root cause was zero audio preprocessing between cpal and
whisper.cpp — every comparable app does at least normalization + VAD.

- `2c08406` chore(tray): silence too_many_arguments on pixel helpers (prep)
- `9cbbffd` feat(audio): resample mic input to 16 kHz mono (rubato FftFixedIn)
- `6363ded` feat(audio): peak-normalize buffer to ~-1 dBFS (one-way boost only)
- `bbf5834` feat(audio): replace RMS trimmer with Silero VAD + hangover smoothing
- `55cfa21` feat(transcribe): tune whisper flags (--no-context, beam=5, temp=0,
            --suppress-blank) + wire cleanup.vocabulary into --prompt

Binary size +18.9 MB (statically-linked ort runtime + 1.8 MB Silero v4 ONNX
model). `cargo clippy -D warnings` clean across all five commits.

Tasks archived at `tasks/done/TASK-09..TASK-12.md`.

## Recent Decisions

- **rdev → CGEventTap** — macOS 26 broke rdev (TSM `dispatch_assert_queue` crash). Direct
  `CGEventTap` via `core-graphics 0.24`. Right Option detected by keycode 0x3D only, no TSM.
- **Homebrew whisper-cpp** — not bundled as Tauri sidecar yet; hardcoded path for now.
- **ggml-base.en** — 141MB, ~130ms on M4 via Metal. Adequate for M1.
- **Window: 380×280** — no custom titlebar, native macOS traffic lights only.
- **Reference, not fork** — built from scratch. Handy/typr/sagascript as references.

## TASK-19 (streaming audio finalizer) — deferred 2026-05-02

Deferred this sprint. Documentation-only completion; no source changes.

**Reason.** TASK-19 explicitly gates on Whisper-vs-finalization timing
evidence ("If Whisper still dominates and audio finalization is small, do
not implement this task"). Two facts make the gate fail-closed right now:

1. TASK-18 (persistent warm Whisper worker) was the dominant-latency
   target this sprint and was itself deferred — `whisper-rs` cmake build
   hangs on macOS 26.x. See `tasks/done/TASK-18-...`. So Whisper still
   dominates per-recording latency by definition.
2. No runtime data has been collected against the TASK-13 stage-timing
   instrumentation (no audio device available to the dispatcher/workers
   this sprint). The `[audio] stage timings (ms): capture_clone=… downmix=…
   resample=… vad=… normalize=… wav_write=… total=…` line in `audio.rs`
   `stop()` exists and is correctly wired, but has not been exercised
   end-to-end with a real microphone.

Optimizing audio finalization before either of those is doubly premature
and would add streaming-pipeline complexity with no measured payoff.

**Re-attempt condition.** Two gates, both required:
- Collect TASK-13 stage timings under realistic recording — at minimum
  one short push-to-talk dictation and one long recording with several
  seconds of leading + trailing silence.
- Only implement TASK-19 if `downmix + resample + vad + normalize +
  wav_write` exceeds Whisper transcription time on the long recording.
  Otherwise re-defer.

Task file moves to `tasks/done/TASK-19-…` with this deferral note as the
proof-of-completion.
