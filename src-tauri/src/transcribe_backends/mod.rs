// transcribe_backends — pluggable transcription backend impls (TASK-58+)
//
// Each sub-module implements `crate::transcribe::TranscriptionBackend`.
// The active backend is selected at startup by `build_backend()` in
// `transcribe.rs` via the `TT_BACKEND` env var or a compile-time const.
//
// Module layout:
//   moonshine  — Moonshine ONNX backend via transcribe-rs (TASK-58)
//   parakeet   — Parakeet ONNX backend via transcribe-rs (TASK-59, TODO)
//
// Feature gate: the `moonshine` feature must be enabled in Cargo.toml for
// the Moonshine impl to compile. This keeps `transcribe-rs` (which pins
// `ort = "=2.0.0-rc.12"`) from conflicting with `vad-rs` (which pins
// `ort = "=2.0.0-rc.9"`) in the default/dev build.

#[cfg(feature = "moonshine")]
pub mod moonshine;
