# TASK-44: Tune `--audio-ctx` for short dictation

## Goal

Find a whisper.cpp `--audio-ctx` value that reduces encoder work on short push-to-talk utterances without degrading short-or-medium-or-long utterance accuracy, with bench numbers across all three length classes to back the choice.

## Context

TurboTalk is push-to-talk dictation; most utterances are well under 10 seconds. whisper.cpp's `--audio-ctx N` flag bounds the encoder context size. Lowering it cuts encoder cost on short clips but can clip the encoder's view of longer ones. The default is 0 (use full 1500 = 30 seconds at 50 Hz).

The decode args are constructed in `src-tauri/src/transcribe.rs:240-257` and currently do **not** pass `--audio-ctx`, so the binary uses default. Adding the flag is a single-line change.

The encoder runs once per clip; the decoder runs per token. So `--audio-ctx` reduces a fixed-per-clip cost. On Metal/CoreML the encoder is already cheap, so the win may be smaller than expected. Measure, don't assume.

Tier 1: name the proof. Bench across short and medium and long samples, not one synthetic short clip.

## In scope

- `src-tauri/src/transcribe.rs` — adding `--audio-ctx N` to the args list
- bench notes (this file or SESSION-STATUS)

## Out of scope

- model swap (separate task)
- decode strategy changes — greedy is a separate task; do not bench audio-ctx and greedy together
- thread tuning (separate task)
- exposing audio-ctx as a Settings UI value — defer
- per-recording-duration audio-ctx scaling — note as follow-up if needed

## Steps

1. Define a bench set:
   - 3 short utterances (<3s) — single word, short phrase, short sentence
   - 3 medium utterances (3–8s) — full sentences, one with names/jargon
   - 2 long utterances (10–20s) — multi-sentence, varied content
   Total: 8 clips. Reuse them for every arm so the comparison is apples-to-apples.
2. Establish baseline. With no `--audio-ctx` flag (current state, default 0/1500), record `[transcribe] whisper took N ms` and exact transcript for each of the 8 clips.
3. Test `--audio-ctx 768`. Add the flag to `transcribe.rs`. Rebuild dev. Repeat the bench set. Record times and transcripts.
4. Test `--audio-ctx 512`. Repeat.
5. Test `--audio-ctx 256`. Repeat.
6. Compare wall times across all four arms. Flag any accuracy regression — pay particular attention to:
   - medium and long utterances at small `--audio-ctx` values (where clipping is most likely)
   - dropped trailing words on long utterances
   - hallucinated repetition (a known whisper.cpp failure mode at very small audio-ctx)
7. Decide:
   - Single value holds across all lengths with measurable speedup → apply it as the new default in `transcribe.rs`.
   - Short utterances benefit but longer utterances regress → leave default. Note "audio-ctx scaling by recording duration" as a follow-up task.
   - No measurable speedup → leave default and document.
8. Record bench numbers and decision in `SESSION-STATUS.md`.

## Success signal

- Wall-time logs across four `--audio-ctx` values for at least 8 utterances each.
- Accuracy notes per arm, especially on medium and long clips.
- A single chosen value (or an explicit "no change" decision with reasoning) recorded.

## Notes

- whisper.cpp internally rounds `--audio-ctx` to multiples of the encoder stride; the actual value used will be visible in the whisper init log. Confirm what the binary actually used, not just what was passed.
- On Metal/CoreML the encoder is already fast — don't be surprised if the wins are smaller than the documented ones from CPU benches.
- If you find `--audio-ctx 256` works for short utterances but breaks long ones, the right follow-up is duration-aware scaling (set audio-ctx based on clip length before spawning whisper-cli), not committing the small value as default.
