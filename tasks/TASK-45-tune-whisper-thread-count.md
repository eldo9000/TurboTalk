# TASK-45: Tune whisper thread count

## Goal

Pick the `-t N` whisper-cli thread count that minimizes wall time on the chosen backend and model, backed by bench numbers across short and medium utterances.

## Context

TurboTalk currently does not pass `-t` to whisper-cli — see `src-tauri/src/transcribe.rs:240-257`. The binary uses its default thread count, which on Apple Silicon may pull in efficiency cores and underperform a tuned setting.

Two effects compete:
- With Metal active, the encoder is on GPU. CPU threads only handle decode and host-side glue. Fewer CPU threads can reduce scheduling overhead.
- With CPU-only fallback, more threads usually wins up to the perf-core count.

Apple Silicon performance vs efficiency cores matter: `-t 4` on an M1/M2/M3 Pro can spill into efficiency cores and tank latency. `-t 2` is often the sweet spot when Metal is doing the heavy work.

Tier 1: name the proof. Bench across utterance lengths and multiple runs per arm, not one synthetic sample.

## In scope

- `src-tauri/src/transcribe.rs` — adding `-t N` to the args list
- bench notes (this file or SESSION-STATUS)

## Out of scope

- model swap (separate task)
- decode strategy (separate task)
- `--audio-ctx` (separate task)
- making thread count user-settable
- thread tuning under CoreML (handled if/when that path lands)

## Steps

1. Confirm Metal is the active backend on the dev build. Run a dictation, capture whisper-cli stderr, confirm the backend init log shows Metal. (If not, the thread bench will be misleading — fix backend selection first.)
2. Define a bench set: 3 short utterances (<3s) + 3 medium (3–8s). Reuse from the audio-ctx bench if it exists.
3. Establish baseline (no `-t` flag — current default). Run each clip 3 times to get a sense of variance. Record `[transcribe] whisper took N ms` for every run.
4. Test `-t 1`. Add the flag to `transcribe.rs`. Rebuild. Run each clip 3 times. Record.
5. Test `-t 2`. Repeat.
6. Test `-t 4`. Repeat.
7. Compare wall times. For each arm, compute median per clip-length-class. Pick the fastest arm whose median wins on both short and medium classes.
8. Decide:
   - Chosen arm materially faster than default (>10% median improvement) → apply as new default in `transcribe.rs`.
   - No clear winner → leave default and document findings.
9. Record bench numbers and decision in `SESSION-STATUS.md`.

## Success signal

- Wall-time logs across four thread settings for at least 6 utterances × 3 runs each.
- Median-per-arm comparison documented.
- A chosen value (or "no change" decision) recorded with reasoning.

## Notes

- Variance on cold model loads is high. Running each clip 3 times per arm filters noise; one run is not enough to make a decision.
- The fastest setting depends on which backend whisper-cli picks at runtime. If Metal isn't actually being used, results don't transfer to a Metal-active build. Always verify backend selection in stderr before trusting the bench.
- On a Mac with both perf and efficiency cores, the OS scheduler may not respect `-t N` exactly the way you'd expect — the binary may still pull in efficiency cores under load. Capture `time` output (user vs system vs wall) if the wall-time numbers look weirdly inconsistent.
- If a persistent whisper worker / model warmth lands before this bench is run, re-run against the warm worker — the optimum thread count for cold-load may differ from steady-state.
