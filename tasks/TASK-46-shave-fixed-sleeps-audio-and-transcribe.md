# TASK-46: Shave fixed sleeps in audio + transcribe

## Goal

Reduce two known fixed sleeps in the dictation pipeline (post-stop audio wait, whisper-cli poll interval) by amounts that measurement proves safe — without breaking trailing-audio capture or cancellation.

## Context

Two small fixed costs are known candidates:

1. **`src-tauri/src/audio.rs:927`** — a 25 ms `std::thread::sleep(Duration::from_millis(25))` after stopping the cpal stream. The wait gives the final audio callback time to land before the WAV is finalized. If shortened too far, the trailing word/consonant could clip.

2. **`src-tauri/src/transcribe.rs:301`** — a 20 ms poll interval inside the whisper-cli wait loop. The poll exists so `abort()` (called from `Recorder::cancel`, TASK-23) can take and kill the child mid-transcription. The whole loop structure is at `transcribe.rs:289-302`. The reason it polls instead of blocking on `wait_with_output()` is documented in the comment block above the loop — preserve that constraint.

Both costs are paid every dictation. Trimming them is small per-dictation but compounds with the per-dictation model load. Cancellation must keep working. Tier 1: name the proof. Bench numbers + a working cancel test, not just "it builds."

## In scope

- `src-tauri/src/audio.rs` — the 25 ms post-stop sleep at line 927
- `src-tauri/src/transcribe.rs` — the 20 ms poll interval at line 301

## Out of scope

- replacing the poll with blocking-wait + signal (a larger refactor — would use `pidfd` on Linux or `kqueue` on macOS to signal child exit; defer)
- model warmth changes
- decode flag changes
- audio capture rate / format changes

## Steps

1. Measure current trailing-audio behavior on the unchanged dev build. Dictate at least 5 utterances ending with a clear final consonant ("test", "cat", "hike", "bark", "stop"). Confirm the trailing consonant survives transcription. This is your before-state.
2. Reduce the `audio.rs:927` sleep from 25 ms to 10 ms. Rebuild dev. Repeat the trailing-consonant test on the same 5+ utterances. If trailing audio is intact across all of them, keep the change. If anything clips on any utterance, revert and try 15 ms.
3. Reduce the `transcribe.rs:301` poll from 20 ms to 5 ms. Rebuild dev. Run a normal dictation — confirm the transcript still lands and `[transcribe] whisper took N ms` log fires.
4. Run the cancel-mid-transcription gesture on the new poll interval. Confirm the subprocess is killed cleanly (no orphan, no panic, no lingering stderr reader thread). Cancel during a long-ish dictation 3+ times to confirm reliability.
5. Bench wall-time impact: dictate 5+ utterances on the new settings vs the old. Capture `[transcribe] whisper took N ms`. The win may be small (single-digit ms per dictation) — record what's actually measured.
6. If both reductions hold, commit them. If either breaks behavior, revert that one (the other can stay) and document why.
7. Record findings in `SESSION-STATUS.md`.

## Success signal

- Trailing audio still captured cleanly with the reduced `audio.rs` sleep across 5+ varied final-consonant utterances.
- Cancel-mid-transcription still kills whisper-cli cleanly with the reduced poll interval, verified across 3+ cancels.
- Bench numbers recorded showing the actual wall-time delta (small is fine; document what's real).

## Notes

- Don't reduce either sleep below ~5 ms — at that scale you're competing with macOS scheduler quanta and the variance dwarfs the win.
- The trailing-consonant test is the load-bearing check on the audio.rs sleep. Don't skip it. A regression here is invisible until a user notices clipped words.
- Orphan check after cancel: `pgrep -fl whisper-cli` after a cancel should return nothing.
- If the 20 ms poll proves load-bearing for some non-obvious reason (e.g. orphan child after kill on certain signal races), the right follow-up is replacing the poll with a proper signaled wait — not making the poll smaller. Document the failure mode and revert.
