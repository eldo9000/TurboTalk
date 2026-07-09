# TASK-58: Moonshine backend via transcribe-rs

## Goal
A `MoonshineBackend: TranscriptionBackend` impl exists. With a hardcoded backend selector flipped to Moonshine and a downloaded Moonshine ONNX model present, holding PTT and saying "hello world" pastes "hello world" into the focused app. The Whisper code path is unaffected when the selector is flipped back. ONNX runtime dylib ships in the macOS release bundle, code-signed.

## Context
TurboTalk is a personal-use macOS push-to-talk dictation utility. Tier 1 product. Read `CLAUDE.md` at repo root and `../../Business-OS/standards/Engineering.md` before starting.

This task assumes TASK-57 has landed: a `TranscriptionBackend` trait exists and `WhisperBackend` is one impl. This task adds a second impl using Moonshine (small ONNX speech recognition model designed for dictation — see <https://github.com/moonshine-ai/moonshine>).

Why Moonshine matters for this app: Moonshine's architecture is not autoregressive in the same way Whisper is, so it does not hallucinate "thanks for watching" on silence. For a PTT dictation tool, this is a structural fix for the original problem TASK-55 and TASK-56 were patching.

The `transcribe-rs` crate (<https://crates.io/crates/transcribe-rs>) provides Rust bindings for several ASR backends including Moonshine and Parakeet, using ONNX Runtime under the hood. ONNX Runtime needs a native dylib bundled with the app. On macOS arm64 this is `libonnxruntime.dylib` (~30 MB). It must be code-signed as part of the existing macOS signing flow.

Models are different from Whisper:
- Whisper uses ggml format files in `~/.config/librewin/turbotalk/models/` (catalog at `src-tauri/src/whisper_models.rs` if it exists, otherwise in `lib.rs` / `settings.rs`)
- Moonshine uses ONNX (one model = multiple `.onnx` files + tokenizer JSON, typically several hundred MB total — the "tiny" variant is smaller)
- Storage layout: keep them separate. Suggest `~/.config/librewin/turbotalk/models/moonshine/<variant>/` so files cluster.

A hardcoded selector is acceptable for this task — read a `TT_BACKEND` env var or a const in `lib.rs`. The proper settings UI lands in TASK-60. The goal here is to prove the backend works end-to-end; UX comes later.

## In scope
- `src-tauri/Cargo.toml` — add `transcribe-rs` (or whichever crate name it publishes under), and `ort` if `transcribe-rs` does not bundle it
- `src-tauri/src/transcribe_backends/moonshine.rs` (new file) — implement `MoonshineBackend: TranscriptionBackend`
- `src-tauri/src/transcribe.rs` — extend `build_backend` to return Moonshine when the selector says so
- ONNX Runtime dylib bundling — add to `tauri.conf.json` `bundle.resources` (or equivalent), wire macOS code signing in CI/release script
- Moonshine model download — a new function alongside the existing Whisper model download. Reuse the progress-event pattern.
- Storage layout under `~/.config/librewin/turbotalk/models/moonshine/`
- Hardcoded backend selector: `TT_BACKEND=moonshine` env var read at startup, or a const for first-pass testing
- `SESSION-STATUS.md` + `TRUTH.md` — one-line each

## Out of scope
- Parakeet backend — that is TASK-59
- Settings UI to switch backends — TASK-60
- Onboarding flow integration — TASK-60
- Tuning Moonshine for accuracy
- Streaming/segment transcription via Moonshine — initial impl can run the full WAV through; the existing `SegmentTranscriber` plumbing can stay Whisper-only for now (note this in code)
- Windows or Linux ONNX runtime bundling — wire the path resolution so it could work, but only ship the macOS arm64 dylib

## Steps
1. Read `CLAUDE.md`, `../../Business-OS/standards/Engineering.md`, `SESSION-STATUS.md`, `TRUTH.md`, current `src-tauri/src/transcribe.rs` (post TASK-57), and `tauri.conf.json`.
2. Look up the current published version of `transcribe-rs` on crates.io. Verify its Moonshine support and what model files it expects. Note any required system deps.
3. Add `transcribe-rs` to `Cargo.toml`. `cargo build` to confirm it pulls and compiles on macOS arm64. Address any ORT linker issues — typically requires `ORT_STRATEGY=download` or a vendored binary.
4. Obtain a Moonshine model. Start with the "tiny" or "base" variant for fastest iteration. Place files manually in `~/.config/librewin/turbotalk/models/moonshine/<variant>/` for first-pass testing. Document the source URLs in code comments.
5. Create `src-tauri/src/transcribe_backends/mod.rs` and `moonshine.rs`. Implement `MoonshineBackend`:
   - `from_config(cfg) -> Result<Self>` — load the ONNX model via `transcribe-rs`, validate the path is inside the allowed models dir (mirror the existing `validate_model_path` allow-list pattern)
   - `transcribe(&self, wav: &Path) -> Result<String>` — feed the WAV to the loaded backend, return the text
   - `abort(&self)` — cancel any in-flight inference if the API allows; otherwise no-op with a tracing warn
   - `model_identity(&self) -> String` — the canonical model path
6. Extend `build_backend(cfg)` in `transcribe.rs` to read the selector and return either `WhisperBackend` or `MoonshineBackend` wrapped in `Arc<dyn TranscriptionBackend>`. For first pass: `std::env::var("TT_BACKEND")` returning `"moonshine"` flips it; anything else is Whisper.
7. Add the Moonshine model download command. Reuse the existing progress-event pattern from `whisper_models.rs` (or wherever Whisper downloads live). Don't wire it into the UI yet — a tauri command is enough for now.
8. Update `tauri.conf.json`: add the ONNX Runtime dylib to bundle resources. Update the macOS signing script (likely in `package.json` scripts or a CI workflow file) to sign the dylib with the existing entitlements/identity.
9. Run `npm run tauri dev` with `TT_BACKEND=moonshine`. Hold PTT, say "hello world", release. Confirm: (a) the transcript appears in the TurboTalk window, (b) the text is approximately "hello world", (c) it pastes into the focused app.
10. Flip `TT_BACKEND` back to unset / "whisper". Confirm Whisper still works end-to-end (regression check).
11. Run `npm run package`. Confirm the resulting `.app` contains `libonnxruntime.dylib` and the dylib is signed (`codesign --verify` on it returns 0).
12. Update `SESSION-STATUS.md` and `TRUTH.md`.
13. Commit with `feat(transcribe): add Moonshine backend (hardcoded selector for testing)`.

## Success signal
- `TT_BACKEND=moonshine npm run tauri dev` + PTT + "hello world" → "hello world" pasted into focused app.
- Unsetting `TT_BACKEND` → Whisper still works end-to-end.
- `npm run package` produces a `.app` containing the signed ONNX Runtime dylib. `codesign --verify --verbose Contents/Frameworks/libonnxruntime.dylib` returns 0.
- `cargo test` exits 0. Existing tests untouched. Moonshine backend has at minimum a path-validation test mirroring the Whisper one.

## Notes
- ORT runtime version compatibility: `transcribe-rs` will pin to a specific ORT version. If you can't pull the matching prebuilt dylib on macOS arm64, you may need `ort` crate's `download-binaries` feature. Document the ORT version in a comment near the dep declaration so future-you knows the matching dylib version.
- Bundle size: ~30 MB ORT + ~80–200 MB Moonshine model. The .app DMG will grow. Note the size delta in the commit message.
- If the Moonshine "tiny" model's accuracy is too low to be useful, fall back to "base". Both are acceptable for the proof; pick whichever transcribes "hello world" cleanly in 1–2 tries.
- The `SegmentTranscriber` streaming queue (`src-tauri/src/transcribe.rs`) currently lives outside backends. Leaving it Whisper-specific is fine for this task — note it explicitly with a `// TODO: generalize` comment so TASK-60 (or a future task) can pick it up.
- Read the README of `transcribe-rs` carefully — there may be platform-specific build flags you need.
