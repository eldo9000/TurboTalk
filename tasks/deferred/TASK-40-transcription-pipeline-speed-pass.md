# TASK-40: Transcription Pipeline Speed Pass

## Goal

Scrape the remaining under-the-hood latency from Turbo Talk's transcription flow without changing UI behavior or user-facing dictation semantics.

Scope is strictly the transcription pipeline after recording: audio handoff, `whisper-cli` invocation, model/backend configuration, model load behavior, and decode flags. Do not spend time on visual/UI optimizations.

## Current Mental Model

Turbo Talk now has configurable mic warmth: Off / 5s / 30s. That keeps the CPAL microphone stream warm between recordings and improves press-to-capture responsiveness. This is valuable, but it is separate from Whisper/model warmth.

The Whisper path still appears to spawn `whisper-cli` per dictation and reload the model each time. That means the biggest remaining latency is likely not microphone startup anymore, but model load + Whisper subprocess + decode.

Relevant files:

- `src-tauri/src/transcribe.rs`
- `src-tauri/src/audio.rs`
- `src-tauri/src/settings.rs`
- `src-tauri/tauri.conf.json`
- `src-tauri/tauri.macos.conf.json`
- `src-tauri/binaries/`

## Optimization 1: Test Greedy Decode Properly

### Hypothesis

The app currently uses beam search with `--beam-size 5`. For short push-to-talk dictation, greedy decoding may be nearly indistinguishable in quality and meaningfully faster.

### Important Detail

Do not only change `--beam-size 5` to `--beam-size 1`.

In whisper.cpp, greedy mode can still use `best_of`, and the CLI default may be `best_of = 5`. For a true low-latency greedy test, use:

```text
--beam-size 1 --best-of 1
```

### Work

- Update the args in `src-tauri/src/transcribe.rs`.
- Test at least:
  - current baseline: `--beam-size 5`
  - greedy: `--beam-size 1 --best-of 1`
  - optional: greedy plus `--no-fallback`

### Acceptance Criteria

- Collect wall-time logs from several short dictations.
- Confirm whether accuracy degradation is acceptable.
- If accuracy drops noticeably, keep this as an optional future setting rather than defaulting it.

## Optimization 2: Test Quantized Turbo Model as Default Speed Path

### Hypothesis

The current configured model is the full `ggml-large-v3-turbo.bin`, about 1.5 GB. The installed quantized model `ggml-large-v3-turbo-q5_0.bin` is much smaller, about 547 MB. Since the app reloads the model per dictation today, smaller model load size may noticeably reduce latency.

### Work

- Benchmark full `large-v3-turbo` against `large-v3-turbo-q5_0`.
- Use the same utterances and same decode flags for both.
- Compare:
  - transcription wall time
  - perceived paste latency
  - obvious accuracy regressions
  - technical vocabulary accuracy

### Acceptance Criteria

- If q5_0 is much faster with minimal quality loss, consider making it the recommended/default download.
- If full model remains default for quality, document q5_0 as the "fast mode" candidate.

## Optimization 3: Verify Bundled Metal Backend

### Hypothesis

The local binary appears capable of using GPU/Metal, but when invoked locally it loaded GGML backend dylibs from Homebrew paths. The packaged app should not depend on Homebrew for fast transcription.

### Work

- Inspect the bundled whisper/ggml dylibs.
- Confirm the release bundle includes all required GGML backend libraries for Metal/CPU on macOS.
- Confirm runtime logs from the packaged app show the intended backend, not an accidental fallback.
- Check `src-tauri/tauri.macos.conf.json`, which currently bundles only a small set of dylibs.

### Acceptance Criteria

- Packaged app works on a clean Mac without Homebrew whisper/ggml libraries.
- Logs clearly show Metal or the intended backend is active.
- No silent CPU-only fallback unless explicitly intended.

## Optimization 4: Persistent Whisper Worker / whisper-server

### Hypothesis

This is likely the biggest remaining win. The app currently keeps the mic stream warm, but not the Whisper model. A persistent worker/server would avoid spawning `whisper-cli` and reloading the model for every dictation.

### Work

- Investigate bundling `whisper-server` or another long-lived whisper.cpp process.
- Keep one model loaded while settings remain unchanged.
- Invalidate/restart the worker when:
  - model path changes
  - vocabulary/prompt changes, if needed
  - transcription backend config changes
  - app quits
- Preserve current cancellation behavior.
- Preserve one-in-flight dictation semantics.

### Acceptance Criteria

- First transcription may remain cold.
- Subsequent transcriptions avoid model reload.
- Settings changes safely restart/rebuild the worker.
- Cancellation still kills or interrupts active transcription cleanly.
- No orphaned sidecar process after app exit.

## Optimization 5: CoreML / Neural Engine Path

### Hypothesis

CoreML can offload Whisper encoder work to Apple Neural Engine and may provide a large speedup on Apple Silicon.

### Important Detail

This is not just "add `-enc-coreml`." For whisper.cpp, CoreML generally requires:

- building whisper.cpp with CoreML support
- generating a compiled encoder model artifact, usually `*-encoder.mlmodelc`
- packaging that artifact with the Whisper model or deriving it during model install
- verifying the exact runtime flags supported by the bundled whisper.cpp version

The currently bundled CLI help did not clearly expose an `-enc-coreml` flag, so this needs verification against the exact whisper.cpp version used.

### Work

- Decide whether to upgrade whisper.cpp or keep current version.
- Build/obtain a CoreML-enabled macOS arm64 binary.
- Generate the matching compiled encoder artifact for supported models.
- Add runtime flag only after confirming the binary supports it.
- Update packaging to include any required `.mlmodelc` directories.
- Benchmark against the Metal path.

### Acceptance Criteria

- App runs without requiring user-installed developer tools.
- CoreML path is measurably faster than current Metal path.
- If CoreML artifacts are model-specific, model download/install flow accounts for that.
- Fallback path is clean if CoreML initialization fails.

## Optimization 6: Tune `--audio-ctx` for Short Dictation

### Hypothesis

The app is optimized for short push-to-talk utterances. Reducing Whisper audio context may reduce encoder work for short clips.

### Work

Test values such as:

```text
--audio-ctx 256
--audio-ctx 512
--audio-ctx 768
--audio-ctx 0
```

### Acceptance Criteria

- Short utterances remain accurate.
- Longer dictations do not degrade badly, or the setting is only applied below a duration threshold.
- Keep default conservative unless benchmark results are clearly positive.

## Optimization 7: Tune Thread Count

### Hypothesis

The CLI default thread count may not be optimal for short dictation, especially with GPU/Metal/CoreML handling heavy work. Fewer threads can sometimes reduce scheduling overhead.

### Work

Test:

```text
-t 1
-t 2
-t 4
```

across short and medium utterances.

### Acceptance Criteria

- Pick the fastest stable value for the chosen backend/model.
- Avoid tuning only for one synthetic sample.

## Optimization 8: Shave Fixed Tail Latency

### Hypothesis

There are small fixed waits in the pipeline that may be safe to reduce after measurement.

Known candidates:

- `audio.rs`: post-stop sleep around 25 ms to allow final audio callback completion.
- `transcribe.rs`: child-process polling sleep around 20 ms.

### Work

- Determine whether the 25 ms audio stop wait can be reduced safely.
- Determine whether child-process polling can use a shorter interval or blocking wait with cancellation support.
- Keep cancellation reliable.

### Acceptance Criteria

- No clipped trailing audio.
- Cancel still works.
- Any win is measured, not assumed.

## Recommended Execution Order

1. Benchmark current baseline with several real dictations.
2. Test `--beam-size 1 --best-of 1`.
3. Test q5_0 model against full turbo.
4. Verify packaged Metal backend correctness.
5. Tune `--audio-ctx` and thread count.
6. Shave fixed sleeps/poll intervals.
7. Implement persistent Whisper worker/server.
8. Investigate CoreML once packaging and model artifacts are clear.

## Notes

Mic warmth is already handled separately. Do not confuse it with Whisper/model warmth.

The highest-confidence quick wins are greedy decode and q5_0 benchmarking. The highest-impact architectural win is persistent Whisper/model warmth. CoreML may be excellent, but it has more packaging complexity and should be treated as a verified backend project rather than a one-line flag change.
