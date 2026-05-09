# TASK-48: CoreML / Neural Engine backend

## Goal

Decide whether shipping a CoreML-enabled whisper.cpp path is worth the packaging cost, by building a CoreML-capable binary, generating model encoder artifacts, benchmarking against the Metal path, and confirming the packaged app works on a clean Mac with CoreML active.

## Context

CoreML can offload Whisper's encoder to the Apple Neural Engine and may be materially faster than Metal on Apple Silicon. **It is not a one-line flag change.** whisper.cpp requires:

- a binary built with `WHISPER_COREML=1` (and possibly `WHISPER_COREML_ALLOW_FALLBACK=1` so dictation still works if CoreML init fails)
- a compiled encoder artifact `<model>-encoder.mlmodelc` next to the model file (this is a directory, not a single file)
- runtime support for the CoreML flag — the exact flag name and behavior must be verified against the bundled whisper.cpp version

The currently bundled `whisper-cli` does not clearly expose `-enc-coreml` in its help, which strongly suggests the bundled binary may need to be replaced. Even if a CoreML-enabled binary is built, CoreML artifacts are model-specific (one `.mlmodelc` per `.bin`), so the model install/download flow in `src-tauri/src/whisper_models.rs` needs to fetch (or generate) the matching artifact.

Packaging: the `.mlmodelc` directory must ship alongside the `.bin` model. If the user downloads a model, the download flow needs progress for both the `.bin` and the matching `.mlmodelc` archive.

Fallback: if CoreML init fails at runtime, transcription must still succeed via Metal. Don't ship a path where a CoreML init error breaks dictation entirely.

Tier 1: name the proof. Bench numbers showing CoreML faster than Metal across varied utterances, plus a clean-Mac install where the packaged DMG dictates with CoreML logged active.

This is the heaviest task in the speed pass. The bulk of the work is whisper.cpp build + packaging, not the runtime flag.

## In scope

- whisper.cpp version decision (keep current or upgrade)
- `src-tauri/binaries/` — replacing `whisper-cli` with a CoreML-enabled build if the current version lacks support
- `src-tauri/src/whisper_models.rs` — model install/download flow to fetch or generate the matching `.mlmodelc`
- `src-tauri/src/transcribe.rs` — adding the CoreML flag to args, plus a fallback path on init failure
- `src-tauri/tauri.conf.json` and `src-tauri/tauri.macos.conf.json` — ensuring the `.mlmodelc` directory ships with the app (or is correctly placed in the user's models dir on download)

## Out of scope

- adding a model that doesn't have a CoreML artifact available
- CoreML on non-Apple-Silicon Macs (the codebase is currently macOS-arm64-only anyway)
- decode flag tuning (separate tasks)
- model swap (separate task)
- generating CoreML artifacts client-side as the primary path — generation requires Python + coremltools and is slow; prefer downloading precomputed `.mlmodelc` archives

## Steps

1. Verify the bundled `whisper-cli` version. Run `<bundled whisper-cli> --help` and capture the output. Confirm whether it lists a CoreML flag (typically `-enc-coreml` or similar) and any required compile-time symbols.
2. If the bundled version lacks CoreML support, build a fresh whisper.cpp from source with CoreML enabled:
   - Clone whisper.cpp at the version you want to ship.
   - Build with `WHISPER_COREML=1 WHISPER_COREML_ALLOW_FALLBACK=1 make -j`.
   - Verify the resulting `whisper-cli` `--help` exposes the CoreML flag.
   - Replace `src-tauri/binaries/whisper-cli-aarch64-apple-darwin`.
3. For each model TurboTalk supports as a recommended default, generate the matching `.mlmodelc`. whisper.cpp ships `models/generate-coreml-model.sh` for this. The output is a `.mlmodelc` directory placed alongside the `.bin`.
4. Update the model install/download flow in `src-tauri/src/whisper_models.rs`:
   - Preferred: download a precomputed `.mlmodelc` archive (zip or tarball) alongside the `.bin`. Extract it next to the model file.
   - Fallback: generate it on first use (slow, requires Python + coremltools — only if precomputed archives are not available).
   - Update progress events to track both files if the UI shows download progress.
5. Add the CoreML flag to whisper-cli args in `src-tauri/src/transcribe.rs:240-257`. Verify the binary behavior by running standalone with the flag against a model that has a `.mlmodelc` and confirming CoreML init succeeds in stderr.
6. Add a runtime fallback path: if the sidecar emits a CoreML init failure on stderr, log a warn and let whisper.cpp fall back internally to Metal (this is what `WHISPER_COREML_ALLOW_FALLBACK=1` is for). Verify by deliberately renaming a `.mlmodelc` directory and confirming dictation still works (slower) via Metal.
7. Update `src-tauri/tauri.conf.json` / `src-tauri/tauri.macos.conf.json` so the `.mlmodelc` directory ships correctly with the app, or is correctly placed by the download flow at runtime.
8. Bench against the established Metal-active baseline: 5+ utterances of varied length. Capture `[transcribe] whisper took N ms` for each. Compare CoreML-active vs Metal-active medians.
9. Build a release DMG: `npm run package`. Install to `/Applications` on a clean Mac (or with Homebrew masked). Dictate. Confirm whisper-cli stderr shows CoreML init success (e.g. `whisper_init_state: loading Core ML model from ...` followed by success).
10. Verify the fallback path on the packaged app: rename the `.mlmodelc` directory in the installed app's bundle (or in the user's models dir if downloaded), restart the app, dictate. Confirm dictation still works via Metal.
11. Decide:
    - CoreML materially faster (>20% median improvement) and install flow is clean → ship it.
    - CoreML marginal (<10%) given the install-flow complexity → defer / document and revert.
    - CoreML faster but install flow is fragile → defer until the install flow can be hardened.
12. Update `SESSION-STATUS.md` and `TRUTH.md` if the recommended backend changes.

## Success signal

- Bench numbers showing CoreML wall time vs. Metal wall time across at least 5 varied utterances, with medians.
- Packaged app on a clean Mac dictates successfully with CoreML logged active.
- Fallback path verified: renaming the `.mlmodelc` and confirming dictation still works via Metal.
- A clear ship / defer decision recorded with reasoning.

## Notes

- Heaviest task in the sprint. If anything blocks you for more than ~30 minutes (build hangs, missing tools, packaging confusion), stop, document the blocker, and defer.
- First-use CoreML init does an ANE warmup that takes several seconds. If a persistent whisper worker is also in place, this hits only the first dictation per session. Without warmth, CoreML may actually be slower than Metal on a per-dictation basis because every dictation pays the init cost. Consider running this task only after the warm-worker path is in place — otherwise the bench numbers will be misleading.
- If the `.mlmodelc` is too large to bundle into the DMG and must be downloaded, the model download UI in `whisper_models.rs` needs progress events for both the `.bin` and the `.mlmodelc` archive.
- Don't spend time generating `.mlmodelc` artifacts client-side as the primary path — `coremltools` is slow and requires a Python toolchain. Prefer hosting precomputed archives somewhere reachable.
- `WHISPER_COREML_ALLOW_FALLBACK=1` at compile time is the difference between "CoreML init failure breaks dictation" and "CoreML init failure logs a warn and falls back to Metal." Always include it.
- The bench for this task only makes sense once Metal backend correctness is independently verified — don't trust a CoreML-vs-Metal comparison if the Metal path itself is silently falling back to CPU.
