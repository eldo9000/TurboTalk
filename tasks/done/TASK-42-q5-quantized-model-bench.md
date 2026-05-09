# TASK-42: q5_0 quantized turbo model bench

## Goal

Decide whether the q5_0 quantized turbo model becomes the recommended default (or a documented "fast mode" candidate), with measured wall-time deltas and accuracy comparison vs. the full `large-v3-turbo` model.

## Context

TurboTalk reloads the whisper model per dictation today — see the module-level deferral note in `src-tauri/src/transcribe.rs:1-16`. Smaller model files mean faster cold loads, which is the dominant cost on a per-dictation basis until persistent worker warmth lands.

The user already has both models installed in `~/.config/librewin/turbotalk/models/`:
- `ggml-large-v3-turbo.bin` (~1.5 GB) — current configured default
- `ggml-large-v3-turbo-q5_0.bin` (~547 MB) — quantized candidate

The model field is configured in `~/.config/librewin/turbotalk/config.toml` (`whisper.model`). The transcription worker rebuilds on model change because `transcribe::invalidate_worker` is called from the settings save path (see `transcribe.rs:386-392`). Switching via the Settings UI triggers this; the next dictation will rebuild against the new model.

The model catalog metadata lives in `src-tauri/src/whisper_models.rs`. If q5_0 wins, update the catalog to mark it recommended-default.

Tier 1: name the proof. Concrete bench numbers, utterance-by-utterance accuracy notes, not "it should be faster."

## In scope

- `~/.config/librewin/turbotalk/config.toml` — `whisper.model` field for the bench (changed via Settings UI to trigger invalidation)
- `src-tauri/src/whisper_models.rs` — model catalog metadata, only if q5_0 is recommended as default
- bench notes (this file or SESSION-STATUS)

## Out of scope

- decode flag changes (separate task)
- worker warmth (separate task)
- adding a UI toggle for "fast vs quality" — defer unless results force it
- bundling additional models into the installer
- the model download flow itself (q5_0 is already installed for this bench)

## Steps

1. With the current `large-v3-turbo` model, dictate at least 5 utterances of varied content:
   - 1 single short word
   - 1 short phrase
   - 1 sentence with a name or jargon term
   - 1 sentence with numbers
   - 1 longer sentence
   Capture `[transcribe] whisper took N ms` and exact output for each.
2. Switch the model to `large-v3-turbo-q5_0` via the Settings UI. Confirm both log lines fire:
   - `[transcribe] worker invalidated`
   - `[transcribe] worker built for model {...q5_0...}`
3. Repeat the same 5 utterances. Record wall times and outputs.
4. Compare: wall-time delta, accuracy regressions, technical-vocabulary handling (names, identifiers, numbers).
5. Decide:
   - q5_0 materially faster, accuracy holds → mark recommended-default in `whisper_models.rs` catalog metadata. Do not change the install flow yet — that's a follow-up.
   - q5_0 faster but accuracy regresses on names/numbers → document as "fast mode" candidate, do not promote.
   - q5_0 not materially faster (<10% delta) → document and revert.
6. Record the bench numbers and decision in `SESSION-STATUS.md` (one line) and update `TRUTH.md` if the recommended model changes.

## Success signal

- Wall-time logs from at least 5 dictations on each model captured.
- Utterance-by-utterance accuracy comparison documented.
- A clear keep / "fast mode" / revert decision recorded with reasoning.
- If recommendation changed: catalog metadata updated and reflected in `TRUTH.md`.

## Notes

- This bench is independent of TASK-41 (greedy). Run each in isolation first; the combination test (greedy + q5_0) is a follow-up only if both win individually.
- The first dictation after a model swap is always cold (full model load). Don't compare a cold q5_0 run vs. a warm large-v3-turbo run — toggle back and forth or compare cold-vs-cold.
- Quantization tends to hurt rare-word and numeric accuracy more than common-word fluency. Pay extra attention to names, identifiers, and digits in the comparison.
