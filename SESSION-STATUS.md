# TurboTalk — Session Status

**Last updated:** 2026-05-03
**Current state:** Streaming-finalizer sprint complete. TASK-22 shipped and
verified — long-recording post-release finalization went from 741.11 ms
(TASK-21 baseline) to 61.94 ms (12.6× speedup) on arm64/macOS 26.4.1.
Resample + VAD now run concurrently with recording, off the user-visible
critical path. Hardening (8/8), dictation-quality (4/4), and
post-quality (TASK-13–17, 20, 21, 22) sprints all closed.

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

None — queue empty. TASK-23 (cancel recording gesture) shipped 2026-05-01.

Open carry-overs are TASK-18's still-deferred warm Whisper backend (gated
on whisper-rs-sys cmake fix or a maintained whisper-server crate; option 3
lifecycle wrapper landed in TASK-20).

## Blockers

None.

## Next action

User's call. Likely candidates:
- Bundle whisper-server alongside whisper-cli to revisit TASK-18 option 2
  (warm model in-process via long-lived sidecar) — would close out the
  remaining warmup gap left from TASK-20 option 3.
- Burn-in / dogfood the streaming pipeline; collect a third evidence
  sample at a later date to confirm the speedup is stable.

## Streaming-finalizer sprint (2026-05-03) — closed

TASK-22 shipped streaming audio finalizer. Concurrent resample + Silero
VAD off the cpal callback thread. Post-release finalization on the same
host as TASK-21:

- Short (1.41s after VAD): 192.49 ms → 49.60 ms (3.9× faster)
- Long  (8.13s after VAD): 781.94 ms → 61.94 ms (12.6× faster)

Both clear the < 250 ms gate and the < 100 ms aspirational target.
Quality preserved (no clipped first/last words).

- `3d13660` feat(audio): streaming audio finalizer — incremental resample + VAD off the callback

Post-landing evidence pasted into `tasks/done/TASK-22-implement-streaming-finalizer.md`.

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

## TASK-21 (streaming-finalizer decision) — 2026-05-03

TASK-21: streaming-finalizer decision = implement (long ratio 35.7%, finalization 741.11 ms; resample-during-silence dominates). → TASK-22 created.

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

## TASK-20 (warm Whisper backend retry) — 2026-05-02 → option 3 landed

Retried TASK-18 with a three-option ladder. Option 1 (`whisper-rs`) gate
**failed** again — same `cmTC_*` cmake hang as the first deferral, killed at
the 300-second budget. Option 2 (`whisper-server`) is **blocked** on a
packaging decision: the binary is not bundled in `src-tauri/binaries/` and
internet downloads were out of scope for this retry. Option 3 (serialized
worker around `whisper-cli`) **landed**: `TranscriptionWorker` in
`src-tauri/src/transcribe.rs` owns binary+model path validation, prompt
state, and a `Mutex` spawn lock; `lib.rs::save_config` invalidates the
cached worker on every save so model swaps and vocabulary edits are
picked up next dictation.

**Warmup is still pending.** Each transcribe call still spawns
`whisper-cli` and reloads the model — the lifecycle wrapper centralizes
the spawn path but does not amortize startup cost. Re-attempt unblocks
on either of (a) `whisper-rs-sys` upstream fix for the macOS 26.x cmake
hang, or (b) deciding to bundle `whisper-server` alongside `whisper-cli`
in `src-tauri/binaries/`.

`cargo build`, `cargo test` (66 passed), and
`cargo clippy -D warnings` all green for `src-tauri`.
