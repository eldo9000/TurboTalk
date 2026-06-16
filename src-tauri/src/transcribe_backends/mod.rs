// transcribe_backends — pluggable transcription backend impls
//
// Each sub-module implements `crate::transcribe::TranscriptionBackend`.
// The active backend is selected at startup by `build_backend()` in
// `transcribe.rs` via the `BackendFamily` enum stored in `settings.Config.backend`
// (persisted as "whisper" / "parakeet" in config.toml; legacy moonshine
// configs are normalized to parakeet on load).
//
// Module layout:
//   parakeet   — Parakeet TDT ONNX backend via transcribe-rs
//
// Feature gate: the `parakeet` feature must be enabled in Cargo.toml for the
// impl to compile. This keeps `transcribe-rs` (which pins `ort =
// "=2.0.0-rc.12"`) from conflicting with `vad-rs` (which pins `ort =
// "=2.0.0-rc.9"`) in the default/dev build.

#[cfg(feature = "parakeet")]
pub mod parakeet;
