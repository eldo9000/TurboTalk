# TASK-21: Collect timing evidence and decide on the streaming audio finalizer

## Goal
A documented decision exists in `SESSION-STATUS.md` and in `tasks/done/TASK-19-optional-streaming-audio-finalizer.md` answering: **does the streaming audio finalizer described in TASK-19 actually pay for the complexity it adds?** The decision is grounded in concrete stage timings from real recordings. If the answer is yes, this task's deliverable is a child task file (TASK-22) sized for a single dispatch. If the answer is no, this task's deliverable is a re-deferral note with the new evidence.

## Context
TurboTalk is a personal-use macOS voice dictation app. The audio pipeline does:
`capture native → downmix → resample to 16 kHz → Silero VAD trim → peak normalize → write WAV`

then hands the WAV to Whisper. TASK-13 added a `tracing::info!` line at the end of `audio.rs::stop()` reporting per-stage milliseconds:
`[audio] stage timings (ms): capture_clone=… downmix=… resample=… vad=… normalize=… wav_write=… total=…`

TASK-19 (streaming finalizer) was deferred 2026-05-02 with two gating reasons:
1. **TASK-18 deferred** — Whisper still dominates per-recording latency, so optimizing finalization first is doubly premature.
2. **No runtime data** — the dispatcher had no audio device, so the timing line in `audio.rs` was never exercised end-to-end.

This task closes gap #2 with a real recording session, then re-evaluates the gate. **Run this task only after TASK-20 has shipped** (so the Whisper baseline reflects whatever warmup state is now in place — comparing finalizer time to a hot Whisper is a different decision than comparing to a cold one).

## Two-phase structure
This task has a hard human checkpoint. Phase 1 is agent-doable. Phase 2 needs a real microphone and a real human pressing the hotkey. Phase 3 is agent-doable again.

| Phase | Who | Deliverable |
|-------|-----|-------------|
| 1. Verify instrumentation | agent | confirms the `[audio] stage timings` log line is wired correctly and prints a fresh sample format spec to `tasks/timing-protocol.md` |
| 2. Record + capture log | user | runs the app, records two dictations (one short, one long with leading/trailing silence), pastes the two timing lines + Whisper duration into `tasks/timing-evidence.md` |
| 3. Analyze + decide | agent | reads the evidence, applies the decision rule, writes the verdict |

The dispatcher should run phase 1, then halt with a clear "USER ACTION REQUIRED" message. The user resumes the dispatcher (or invokes `/triage` again) once they've pasted the evidence file.

## In scope
- `src-tauri/src/audio.rs` — read-only verification that the stage-timing log line exists and is wired to fire on every accepted recording
- `src-tauri/src/transcribe.rs` — read-only verification that there's a corresponding "transcription took N ms" log line (or add one if missing — small additive change)
- `tasks/timing-protocol.md` — new file, agent-written, describes exactly what the user should do in phase 2
- `tasks/timing-evidence.md` — new file, user-written in phase 2, agent-read in phase 3
- `tasks/done/TASK-19-optional-streaming-audio-finalizer.md` — append a "Re-evaluation $(date)" section
- `SESSION-STATUS.md` — one-line update under the appropriate sprint heading
- **Phase 3 only, only if the decision is "implement":** create `tasks/TASK-22-implement-streaming-finalizer.md` from a template (do not implement the finalizer in this task)

## Out of scope
- Implementing the streaming finalizer itself (that's TASK-22, only created if phase 3 decides "yes")
- Changing the existing pipeline order or stages
- Optimizing the resampler or VAD inline (TASK-17 already cached the VAD session)
- Adding compression codecs or WAV format changes
- Touching anything covered by TASK-20 (warm Whisper)

## Decision rule (applied in phase 3)
Implement the streaming finalizer **only if both** are true on the long-recording sample:
1. `downmix + resample + vad + normalize + wav_write` (the post-release finalization sum) exceeds 30% of Whisper's transcription time, AND
2. The absolute finalization time exceeds 250 ms.

The 30% threshold prevents optimizing something that's already dwarfed by Whisper. The 250ms floor prevents optimizing tens of ms of work into a streaming pipeline that adds far more architectural cost than it saves wall-time.

If either condition fails, **re-defer** TASK-19 with the new evidence. Don't implement.

## Steps

### Phase 1 — agent
1. Read `src-tauri/src/audio.rs` and confirm the `[audio] stage timings (ms): …` line still exists and is emitted in `stop()` on every accepted recording (not skipped on Discard paths, but also not emitted before audio is accepted — match what TASK-13 specified).
2. Read `src-tauri/src/transcribe.rs` and confirm a transcription-duration log line exists. If not, add a minimal one: capture `Instant::now()` before the worker call and emit `tracing::info!("[transcribe] whisper took {} ms", elapsed.as_millis())` after. This is a small additive change; do not refactor.
3. Run `cargo build` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — clean for any code touched.
4. Write `tasks/timing-protocol.md` — about 30 lines — covering:
   - exactly what to launch (`npm run tauri dev` or a built bundle)
   - which terminal to watch (the one running `npm run tauri dev` will print `tracing::info!` lines)
   - two recording scenarios:
     - **Short:** hold hotkey, say "the quick brown fox jumps over the lazy dog", release. ~3s of audio.
     - **Long with silence:** hold hotkey, wait ~2s in silence, say "this is the long recording with silence at both ends", wait ~3s in silence, release. ~10s of audio.
   - what to copy: the `[audio] stage timings` line and the `[transcribe] whisper took` line for each scenario, plus the host info (`uname -m`, `sw_vers`).
   - where to paste: a template `tasks/timing-evidence.md` that the protocol creates (or instructs the user to create) with clearly labeled sections for short/long.
5. Halt. Print exactly:
   ```
   [TASK-21] Phase 1 complete. USER ACTION REQUIRED.
   Read tasks/timing-protocol.md, run the two recordings, paste log lines
   into tasks/timing-evidence.md, then resume this task.
   ```
   Return outcome=halt with notes pointing to the protocol file.

### Phase 2 — user (no agent action; documented here for clarity)
The user follows `tasks/timing-protocol.md`, captures two `[audio] stage timings` lines and two `[transcribe] whisper took` lines, and writes them into `tasks/timing-evidence.md`. Then re-dispatches this task or runs `/triage tasks/TASK-21-streaming-finalizer-decision.md` to enter phase 3.

### Phase 3 — agent
6. Read `tasks/timing-evidence.md`. If it doesn't exist or is empty, halt with the same phase-1 message — phase 2 hasn't happened yet.
7. Parse out the four numbers: short-finalization-sum, short-whisper-ms, long-finalization-sum, long-whisper-ms. Compute the long-recording ratio: `long_finalization / long_whisper`.
8. Apply the decision rule from above. Two outcomes:
   - **Implement:** create `tasks/TASK-22-implement-streaming-finalizer.md` using TASK-19's archived spec as the base — copy in the streaming-finalizer steps, update with the concrete numbers from this evidence, and pin a perf target (`reduce long-recording finalization from <measured> ms to <target> ms`). Do NOT implement in this task; only the file.
   - **Re-defer:** do not create TASK-22.
9. Append a "Re-evaluation YYYY-MM-DD" section to `tasks/done/TASK-19-optional-streaming-audio-finalizer.md` containing the evidence numbers, the computed ratio, and the verdict. If implementing, the section ends with "→ TASK-22 created"; if re-deferring, it ends with the new gate condition (e.g. "re-evaluate when long-recording finalization exceeds N ms or whisper drops below M ms").
10. Update `SESSION-STATUS.md` with a single line under the active sprint heading: `TASK-21: streaming-finalizer decision = <implement | re-deferred> based on <ratio>%/<finalization>ms long-recording evidence`.

## Success signal
- `tasks/timing-protocol.md` exists and is unambiguous — a user can follow it without asking questions.
- After phase 2: `tasks/timing-evidence.md` exists with concrete numbers.
- After phase 3: TASK-19's archive file has a fresh "Re-evaluation" section with numbers, and either (a) `tasks/TASK-22-implement-streaming-finalizer.md` exists, or (b) the re-deferral note is in place.
- `SESSION-STATUS.md` reflects the verdict.

## Notes
- **Do not implement the finalizer in this task even if the decision is "implement"** — that's TASK-22's job, in its own dispatch. This task is decision-making only.
- The 30% / 250ms thresholds are deliberately conservative because the streaming finalizer adds architectural complexity (worker boundary, callback discipline, prefill/hangover logic). Cheap wins should be skipped; only real bottlenecks justify it.
- If the user reports back that the timing line never appeared in their terminal, that's a phase-1 instrumentation bug, not a phase-2 user error. Re-enter phase 1 and fix.
- If `tasks/timing-evidence.md` exists but is malformed (couldn't parse 4 numbers), halt and ask the user to fix the format rather than guessing.
- Multi-agent review reference: original deferred task at `tasks/done/TASK-19-optional-streaming-audio-finalizer.md`; full deferral rationale in `SESSION-STATUS.md` lines 80–108.
