# TASK-41: Greedy decode A/B test

## Goal

Decide whether `--beam-size 1 --best-of 1` becomes the new whisper-cli decode default for TurboTalk, with measured baseline and post-change wall times across several real dictations and an explicit accuracy comparison.

## Context

TurboTalk is a personal-use macOS push-to-talk dictation utility. The transcription path spawns `whisper-cli` per dictation (no model warmth — see the module-level deferral note in `src-tauri/src/transcribe.rs:1-16`). Current decode args are set in `src-tauri/src/transcribe.rs:240-257`:

```
-m <model> -f <wav> -otxt -np -nt -l en -mc 0 --beam-size 5 --temperature 0 --suppress-nst
```

In whisper.cpp, "greedy" mode requires more than dropping `--beam-size`. The CLI's default `best_of` is 5, so even with `--beam-size 1` you still pay 5 candidate decodes per segment. A true greedy A/B must set both: `--beam-size 1 --best-of 1`. Optionally also test `--no-fallback` (skips temperature fallback on no-speech) as a third arm.

This is the cheapest possible win in the speed pass — single-line change, no packaging impact. If accuracy holds, ship it; if it regresses, either revert or expose as a user-settable "fast mode" later.

Tier 1 (TurboTalk operates at Tier 1 per the operating model): name the proof. Wall-time logs and accuracy spot-check, not "it compiles."

## In scope

- `src-tauri/src/transcribe.rs` — decode args
- `SESSION-STATUS.md` and `TRUTH.md` updates if the decision changes user-facing behavior
- bench notes (append to this file or to SESSION-STATUS)

## Out of scope

- model swap (a separate task in this sprint covers q5_0)
- worker warmth (separate task)
- CoreML (separate task)
- `--audio-ctx` / thread tuning (separate tasks)
- exposing decode strategy as a Settings UI toggle — defer unless results are borderline

## Steps

1. Build dev mode: `npm run tauri dev`. Confirm dictation works end-to-end on the current build.
2. Establish baseline. With unchanged `--beam-size 5`, dictate at least 5 utterances of varied content:
   - 1 single short word
   - 1 short phrase (3–5 words)
   - 1 sentence containing a name or jargon term from `cleanup.vocabulary`
   - 1 sentence containing numbers
   - 1 longer sentence (15+ words)
   For each, capture the `[transcribe] whisper took N ms` log line and the exact transcribed output. Record both.
3. Edit `src-tauri/src/transcribe.rs:240-257` to use `--beam-size 1 --best-of 1` (replace the existing `--beam-size 5`, add `--best-of 1`).
4. Rebuild and repeat the same 5 utterances. Record wall times and exact outputs.
5. (Optional third arm) Add `--no-fallback`. Repeat the bench.
6. Compare arms: wall-time delta, accuracy regressions on names/jargon/numbers, hallucinated repetition, dropped trailing tokens.
7. Decide:
   - Accuracy holds → keep greedy as the default.
   - Accuracy regresses materially → revert.
   - Borderline → revert and note "fast mode" candidate as follow-up.
8. Record the bench numbers and the decision in `SESSION-STATUS.md` (one line) and inline in the `transcribe.rs` arg-list comment if defaults changed.

## Success signal

- Wall-time logs from at least 5 dictations on each arm captured in this task file or SESSION-STATUS.
- Utterance-by-utterance accuracy comparison documented.
- A clear keep / revert / conditional decision recorded with reasoning.

## Notes

- whisper.cpp's `--no-fallback` skips temperature fallback on no-speech; greedy + no-fallback is the most aggressive low-latency arm but loses one of whisper's safety nets. Test carefully.
- If the change defaults greedy, leave the comment block above the args list updated to reflect the new tuning rationale.
- Don't over-trust a single bench run — variance is high on cold model loads. Run each arm 5+ times.
