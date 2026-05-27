## SVE-cmake-probe-hang
check_cxx_source_runs for ARM SVE hangs on macOS arm64 (Apple M4/M3/M2/M1 have no SVE).
Fix: patch check_cxx_source_runs → check_cxx_source_compiles in ggml/src/ggml-cpu/CMakeLists.txt.
Applies to: whisper.cpp cmake, any ggml-based cmake build on macOS arm64.

## CoreML-dyld-init-hang
Building whisper.cpp with `WHISPER_COREML=1` links `libwhisper.coreml.dylib` into
`libwhisper.1.dylib`, which pulls in `CoreML.framework` at **dyld load time** — before
`main()`. On Apple Silicon this can block process startup for ~60 s on every cold start,
even when no `.mlmodelc` encoder artifact is present and CoreML is never used.

**Symptom:** `whisper-server` or any binary loading the CoreML-linked `libwhisper.1.dylib`
appears hung immediately after spawn; no transcription logs yet.

**Mitigation (shipped):**
- Default bundle uses **Metal-only** Homebrew whisper (`npm run refresh-whisper-server`).
- `scripts/preflight.mjs` and `scripts/refresh-whisper-server.mjs` reject binaries whose
  dylib chain references `CoreML.framework` or `libwhisper.coreml.dylib`.
- Optional CoreML path (TASK-48 phase 2) must use a **separate sidecar** spawned only when
  the user opts in — never replace the default Metal `libwhisper.1.dylib`.

**Re-attempt gate:** Documented optional sidecar design in `docs/reference/COREML-BLOCKER.md`.
Do not merge a CoreML-linked default sidecar until a timed proof shows dyld init ≤ 2 s without
`.mlmodelc`, or the opt-in sidecar path is implemented and bench-validated.

**Related upstream:** whisper.cpp issues on ANECompilerService hangs during CoreML *model*
load (distinct from this dyld-init class, but same ANE subsystem).
