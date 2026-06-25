// transcribe_backends — transcription backend implementations
//
// Each sub-module implements `crate::transcribe::TranscriptionBackend`.
// The active backend is selected at startup by `build_backend()` in
// `transcribe.rs` via the `BackendFamily` enum stored in `settings.Config.backend`
// (persisted as "whisper" / "parakeet" in config.toml).
//
// Module layout:
//   parakeet   — Parakeet TDT ONNX backend via transcribe-rs
//
// Feature gate: the `parakeet` feature must be enabled in Cargo.toml for the
// impl to compile.

#[cfg(feature = "parakeet")]
pub mod parakeet;
