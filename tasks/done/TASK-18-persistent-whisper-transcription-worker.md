# TASK-18: Replace per-recording Whisper startup with a persistent transcription worker

## Goal
TurboTalk avoids paying unnecessary Whisper process/model startup cost for every dictation. A long-lived transcription backend should keep the selected model warm when practical, while preserving the current WAV handoff behavior and security guards.

## Context
`src-tauri/src/transcribe.rs` currently runs `whisper-cli` once per recording:

1. validate sidecar binary path
2. validate model path
3. spawn `whisper-cli`
4. write/read `.txt`
5. delete `.txt`

That is simple and correct, but likely leaves the biggest latency win on the table. Codec changes will not matter much compared with repeatedly spawning Whisper and loading the model/Metal context.

## In scope
- Measure current Whisper latency using TASK-13/TASK-15 stage timings.
- Implement a persistent transcription worker if feasible.
- Preserve:
  - model path validation
  - sidecar/path allowlist behavior
  - vocabulary prompt behavior
  - tuned dictation flags
  - one in-flight job semantics from TASK-14
- Add fallback behavior if the persistent worker fails to initialize.

## Out of scope
- Changing audio file format.
- Changing model catalog UI.
- Network transcription.
- Multi-job queueing.
- Changing cleanup behavior.

## Implementation options
Evaluate in this order:

1. **Native whisper.cpp library binding**
   - Best long-term latency if already practical in the repo.
   - Keeps model loaded in-process.
   - More build complexity.

2. **Long-lived sidecar/server mode**
   - Acceptable if whisper.cpp provides a local server or stdin-friendly mode that can process repeated files without reloading the model.
   - Must remain local-only.

3. **Serialized worker around `whisper-cli`**
   - Least benefit because it still spawns per recording.
   - Still useful for isolating transcription ownership, but do not claim it solved model warmup.

## Steps
1. Read `src-tauri/src/transcribe.rs`, `src-tauri/src/settings.rs`, and the lifecycle code from TASK-14/TASK-15.
2. Record baseline timing for:
   - Whisper stage total
   - first dictation after app launch
   - second and third dictations in the same app session
3. Choose the simplest backend that genuinely avoids repeated model startup.
4. Add a `TranscriptionWorker` abstraction:
   - initialized at app setup or lazily on first transcription.
   - owns model path and prompt settings.
   - reloads if the selected model changes.
   - processes one job at a time.
5. Keep `transcribe.rs` path validation functions. Do not loosen the allowlist.
6. Preserve output behavior: return raw text to the caller introduced in TASK-15.
7. Add error handling:
   - if persistent worker startup fails, either return a clear `transcript-error` or temporarily fall back to `whisper-cli`.
   - if falling back, log that the warm worker is disabled.
8. Update settings save/model-change path if the worker needs invalidation when `cfg.whisper.model` changes.
9. Run:
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
10. Manual test:
   - first recording after app launch
   - several back-to-back recordings
   - model switch, then recording
   - missing/invalid model path error

## Success signal
- Repeated dictations no longer pay full model/process startup cost, or the task records why the selected backend cannot provide that yet.
- Normal dictation still works end-to-end.
- Invalid model paths are still rejected.
- Stage timing shows improved repeated-dictation latency.
- Tests and clippy pass.

## Notes
- This is the highest-upside optimization. Do not spend energy on compressed audio before doing this.
- Keep the existing CLI path as a fallback until the persistent backend has real manual proof.

## Deferral — 2026-05-02 (re-confirmed after retry)

Warm-worker route attempted twice via `whisper-rs` 0.16 with the `metal` feature.
Both attempts hung in the same place: `whisper-rs-sys`'s build script invokes
`cmake -Wdev --debug-output ... -DCMAKE_VERBOSE_MAKEFILE:BOOL=ON` which spawns
a `TryCompile` probe (`cmTC_*`). On this macOS 26.4.1 (Tahoe) host the probe
binary runs but never terminates — wall time 11+ min, ~6 min CPU, no
`cargo check` progress past the `whisper-rs-sys` build.rs step.

Confirmed reproducibility on retry after disk space was freed (28+ GiB
available on the second attempt), so this is not a downstream symptom of the
earlier disk-full state — it is a real environmental block on the
cmake/sysroot interaction with `--target=arm64-apple-macosx
-mmacosx-version-min=26.4.1`. The verbose/debug cmake flags are baked into
`whisper-rs-sys`'s build.rs; we cannot suppress them without forking.

Per task notes ("Keep the existing CLI path as a fallback until the persistent
backend has real manual proof"), the dependency was reverted and the
existing `whisper-cli` per-recording path remains in place.

Re-attempt conditions (any one):
- Upstream `whisper-rs-sys` removes `--debug-output` / verbose cmake
  invocation, or fixes the `cmTC_*` hang on macOS 26.x.
- macOS 26.x point release fixes the cmake compiler-detection hang.
- A maintained crate emerges that wraps `whisper.cpp`'s long-lived server
  binary (option 2) without bringing the C++ build under cmake into our
  cargo graph.
- Acceptable to drop down to option 3 (serialized worker around `whisper-cli`)
  if the lifecycle ownership benefit is wanted independent of warmup.

