// transcribe_backends — pluggable transcription backend impls (TASK-58+)
//
// Each sub-module implements `crate::transcribe::TranscriptionBackend`.
// The active backend is selected at startup by `build_backend()` in
// `transcribe.rs` via the `BackendFamily` enum stored in `settings.Config.backend`
// (persisted as "whisper" / "moonshine" / "parakeet" in config.toml).
// The old TT_BACKEND env var was removed in TASK-60.
//
// Module layout:
//   moonshine  — Moonshine ONNX backend via transcribe-rs (TASK-58)
//   parakeet   — Parakeet TDT ONNX backend via transcribe-rs (TASK-59)
//
// Feature gate: the `moonshine` and `parakeet` features must be enabled in
// Cargo.toml for those impls to compile. This keeps `transcribe-rs` (which
// pins `ort = "=2.0.0-rc.12"`) from conflicting with `vad-rs` (which pins
// `ort = "=2.0.0-rc.9"`) in the default/dev build.

#[cfg(feature = "moonshine")]
pub mod moonshine;

#[cfg(feature = "parakeet")]
pub mod parakeet;
