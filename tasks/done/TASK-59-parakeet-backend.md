# TASK-59: Parakeet backend via transcribe-rs

## Goal
A `ParakeetBackend: TranscriptionBackend` impl exists. With the hardcoded selector flipped to `parakeet` and a downloaded Parakeet ONNX model present, holding PTT and saying "hello world" pastes "hello world" into the focused app. The Whisper and Moonshine code paths still work when the selector is flipped to either of them.

## Context
TurboTalk is a personal-use macOS push-to-talk dictation utility. Tier 1 product. Read `CLAUDE.md` at repo root and `../../Business-OS/standards/Engineering.md` before starting.

This task assumes TASK-57 (trait abstraction) and TASK-58 (Moonshine backend) have landed. ONNX Runtime dylib is already bundled and signed from TASK-58 — Parakeet uses the same runtime, so most of the plumbing already exists. This task is largely a pattern-match against TASK-58 with a different model loader and storage path.

NVIDIA Parakeet (specifically Parakeet TDT) is an English-only ASR model with extremely high throughput. Architecture: CTC/TDT, not autoregressive — so like Moonshine, it does not generate text from a language-model prior in the same way Whisper does, and is not prone to "thanks for watching"-style hallucination.

The `transcribe-rs` crate supports Parakeet alongside Moonshine, using the same ONNX Runtime under the hood. Models live at `~/.config/librewin/turbotalk/models/parakeet/<variant>/`.

## In scope
- `src-tauri/src/transcribe_backends/parakeet.rs` (new file) — implement `ParakeetBackend: TranscriptionBackend`
- `src-tauri/src/transcribe.rs` — extend `build_backend` to return Parakeet when the selector says `parakeet`
- Parakeet model download — new function alongside the Whisper and Moonshine downloads
- Storage layout under `~/.config/librewin/turbotalk/models/parakeet/`
- Extend the hardcoded backend selector (env var or const) to recognize `parakeet`
- `SESSION-STATUS.md` + `TRUTH.md` — one-line each

## Out of scope
- Settings UI / onboarding integration — TASK-60
- Streaming/segment transcription via Parakeet — initial impl runs the full WAV through; existing `SegmentTranscriber` stays Whisper-only
- Tuning Parakeet for accuracy
- Windows or Linux model bundling — wire path resolution but ship only the macOS arm64 path

## Steps
1. Read `CLAUDE.md`, `../../Business-OS/standards/Engineering.md`, `SESSION-STATUS.md`, `TRUTH.md`, the now-landed Moonshine impl at `src-tauri/src/transcribe_backends/moonshine.rs`. The Parakeet impl should mirror its structure.
2. Confirm `transcribe-rs` supports Parakeet TDT and which model variant to start with. Use the smallest variant that's known to work cleanly.
3. Download a Parakeet ONNX model manually into `~/.config/librewin/turbotalk/models/parakeet/<variant>/` for first-pass testing. Document the source URL in code comments.
4. Create `src-tauri/src/transcribe_backends/parakeet.rs`. Implement `ParakeetBackend`:
   - `from_config(cfg) -> Result<Self>` — load the ONNX model via `transcribe-rs`, allow-list-validate the model path
   - `transcribe(&self, wav: &Path) -> Result<String>` — feed the WAV, return text
   - `abort(&self)` — same shape as Moonshine impl
   - `model_identity(&self) -> String` — canonical model path
5. Extend `build_backend` in `transcribe.rs`: `"parakeet"` returns `ParakeetBackend`; other selector values unchanged.
6. Add the Parakeet model download command, mirroring the Moonshine one from TASK-58.
7. Run `npm run tauri dev` with `TT_BACKEND=parakeet`. Hold PTT, say "hello world", release. Confirm transcript is approximately "hello world", pastes into the focused app.
8. Flip the selector through all three values in turn:
   - `unset` → Whisper still works.
   - `moonshine` → Moonshine still works.
   - `parakeet` → Parakeet works.
9. Run `cargo test`. Add a path-validation test for Parakeet mirroring the Moonshine one. All other tests untouched.
10. Update `SESSION-STATUS.md` and `TRUTH.md`.
11. Commit with `feat(transcribe): add Parakeet backend (hardcoded selector for testing)`.

## Success signal
- `TT_BACKEND=parakeet npm run tauri dev` + PTT + "hello world" → "hello world" pasted into focused app.
- All three backends (Whisper, Moonshine, Parakeet) usable end-to-end by flipping the env var. No backend's code path breaks the others.
- `cargo test` exits 0.

## Notes
- This task should be the smallest of the four Phase 3 sessions because TASK-58 already paved the ONNX runtime / bundling / dylib-signing road. If you find yourself doing infrastructure work here that wasn't needed for Moonshine, stop and reconsider — it probably belongs in a separate task or back-ported into TASK-58.
- If `transcribe-rs`'s Parakeet API differs notably from its Moonshine API (different preprocessing, different output shape), that's a sign to either factor a helper inside `transcribe_backends/` or accept the divergence with a comment. Don't prematurely generalize.
- If Parakeet sometimes returns lowercased / unpunctuated output (CTC models often do), accept that as-is — punctuation/casing is a cleanup-layer concern handled later by the Chaperone Layer (`src-tauri/src/cleanup.rs`).
