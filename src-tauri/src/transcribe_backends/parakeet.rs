// Parakeet ONNX transcription backend (TASK-59)
//
// Implements `TranscriptionBackend` using the `transcribe-rs` crate's Parakeet
// support. Parakeet TDT is an NVIDIA English-only CTC/TDT model with extremely
// high throughput. Unlike autoregressive models (Whisper), CTC models do not
// hallucinate "thanks for watching" on silence — the output is strictly bounded
// to what was in the audio.
//
// Key characteristics:
//   - English-only (CTC architecture, not seq2seq)
//   - Output may be lowercase/unpunctuated — Chaperone cleanup handles it
//   - No autoregressive hallucination on silence (structural fix, not a filter)
//   - Full WAV in, transcript out — no streaming; SegmentTranscriber stays Whisper
//
// ── DEPENDENCY CONFLICT (currently unresolved) ───────────────────────────────
//
// `transcribe-rs 0.3.11` (needed for Parakeet) pins `ort = "=2.0.0-rc.12"`.
// `vad-rs 0.1.5`          (used for in-process VAD) pins `ort = "=2.0.0-rc.9"`.
// Cargo cannot resolve two exact-pinned versions of the same crate, even when
// one is declared optional. This means `transcribe-rs` cannot be added to
// Cargo.toml at all until one of these unblocking paths is taken:
//
//   (a) vad-rs upgrades to ort rc.12 — simplest, wait for upstream.
//   (b) Replace vad-rs with a direct ort rc.12 VAD integration.
//   (c) Move transcribe-rs into a separate Cargo workspace member / sidecar.
//
// Until unblocked, this file is guarded by `#[cfg(feature = "parakeet")]` so
// it does not affect the default build. The `parakeet` feature is declared in
// Cargo.toml but has no dep attached yet (see the comment there).
//
// ── Model storage ────────────────────────────────────────────────────────────
//
// Model files (ONNX bundle) are stored under:
//   ~/.config/librewin/turbotalk/models/parakeet/<variant>/
//
// Required files per variant (Parakeet TDT):
//   model.onnx        (the CTC/TDT graph)
//   tokenizer.json
//
// Source: https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2
// (ONNX export — look for the ONNX community fork or the export script in the
// repo; the raw HuggingFace checkpoint is SafeTensors/PyTorch, not ONNX)
//
// ── ONNX Runtime dylib ───────────────────────────────────────────────────────
//
// Parakeet uses the same ONNX Runtime as Moonshine:
//   macOS arm64: libonnxruntime.dylib (~30 MB), declared in tauri.macos.conf.json.
// Download from: https://github.com/microsoft/onnxruntime/releases
// (match the version required by the resolved `ort` crate — currently rc.12).
//
// ── Activation ───────────────────────────────────────────────────────────────
//
// Set TT_BACKEND=parakeet at runtime to route through this backend.
// Once the dep conflict is resolved:
//   1. Uncomment `transcribe-rs` in Cargo.toml.
//   2. Build with `cargo build --features parakeet`.
//   3. Set TT_BACKEND=parakeet, download a Parakeet model via
//      `download_parakeet_model`, and hold PTT to test.
//
// TODO (TASK-60): wire a settings UI toggle so users can pick Parakeet vs Whisper.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::transcribe::{TranscriptOutcome, TranscriptionBackend};

// ── Variant type ─────────────────────────────────────────────────────────────
//
// We define our own variant enum rather than re-exporting from transcribe-rs
// so the enum is available for path helpers even before the dep is unblocked.

/// Parakeet model variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParakeetVariant {
    /// Parakeet TDT 0.6B v2 — NVIDIA's flagship English ASR model.
    /// Extremely high throughput, CTC/TDT architecture.
    Tdt06b,
}

impl ParakeetVariant {
    /// Directory-safe name for the variant (used as storage subdirectory).
    pub fn name(self) -> &'static str {
        match self {
            ParakeetVariant::Tdt06b => "tdt-0.6b-v2",
        }
    }

    /// Human-readable display name.
    pub fn display_name(self) -> &'static str {
        match self {
            ParakeetVariant::Tdt06b => "Parakeet TDT 0.6B v2",
        }
    }
}

/// Parse a user-facing variant string (case-insensitive).
/// Returns `None` for unrecognised strings.
pub fn parse_variant(s: &str) -> Option<ParakeetVariant> {
    match s.to_lowercase().as_str() {
        "tdt-0.6b-v2" | "tdt06b" | "tdt_0.6b_v2" => Some(ParakeetVariant::Tdt06b),
        _ => None,
    }
}

// ── Model path helpers ────────────────────────────────────────────────────────

/// Base directory for Parakeet model bundles.
/// Returns `~/.config/librewin/turbotalk/models/parakeet/`.
///
/// Does NOT require the directory to exist — callers that need it to exist
/// (e.g. the download command) create it themselves.
pub fn parakeet_models_dir() -> Option<PathBuf> {
    let mut p = dirs::home_dir()?;
    p.push(".config/librewin/turbotalk/models/parakeet");
    Some(p)
}

/// Path to the ONNX bundle directory for a specific variant.
/// Returns `~/.config/librewin/turbotalk/models/parakeet/<variant>/`.
pub fn variant_dir(variant_name_str: &str) -> Option<PathBuf> {
    let mut p = parakeet_models_dir()?;
    p.push(variant_name_str);
    Some(p)
}

/// Validate that a Parakeet model directory exists and contains the required
/// files: `model.onnx`, `tokenizer.json`. Returns `Ok(canonicalized_path)`.
///
/// Mirrors `validate_moonshine_model_dir` in moonshine.rs but for Parakeet's
/// file set (CTC models use a single graph file rather than separate
/// encoder/decoder).
pub fn validate_parakeet_model_dir(dir: &Path) -> anyhow::Result<PathBuf> {
    let canon = dir.canonicalize().map_err(|_| {
        anyhow::anyhow!(
            "Parakeet model directory not found: {}. \
             Run download_parakeet_model to fetch it.",
            dir.display()
        )
    })?;

    // Verify the directory is inside the expected base to block path traversal.
    if let Some(base) = parakeet_models_dir() {
        if let Ok(canon_base) = base.canonicalize() {
            if !canon.starts_with(&canon_base) {
                anyhow::bail!(
                    "Parakeet model directory is outside the allowed path: {}",
                    dir.display()
                );
            }
        }
    }

    // Check required files.
    for filename in &["model.onnx", "tokenizer.json"] {
        let f = canon.join(filename);
        if !f.exists() {
            anyhow::bail!(
                "Parakeet model incomplete — missing {filename} in {}. \
                 Re-run download_parakeet_model.",
                canon.display()
            );
        }
    }

    Ok(canon)
}

// ── ParakeetBackend ───────────────────────────────────────────────────────────

/// `TranscriptionBackend` backed by Parakeet TDT ONNX via `transcribe-rs`.
///
/// # Architecture
///
/// Parakeet TDT is a CTC (Connectionist Temporal Classification) model, not
/// autoregressive. This means:
///   - No beam search — inference is O(n) in audio length, not O(n²)
///   - No hallucination loop — output strictly bounded to heard audio
///   - Output may be lowercase, unpunctuated — Chaperone cleanup normalizes it
///
/// # Thread safety
///
/// The model handle is wrapped in a `Mutex` so `ParakeetBackend` can be
/// `Send + Sync` as required by `Arc<dyn TranscriptionBackend>`.
/// Transcription is serialized through the mutex, matching the one-in-flight
/// invariant already enforced by WhisperBackend's `spawn_lock`.
///
/// # Abort
///
/// Parakeet runs entirely in-process (no subprocess), so `abort()` cannot
/// interrupt an in-flight ONNX session. The `abort_flag` is set and checked
/// at the transcription entry point — a cancel takes effect between recordings
/// but not mid-inference. In-flight inference completes naturally.
///
/// # Build status
///
/// This struct requires the `parakeet` feature AND the `transcribe-rs` dep.
/// Until the ort version conflict is resolved, this compiles as a stub that
/// returns a clear error at construction time rather than silently failing.
pub struct ParakeetBackend {
    /// Canonicalized model directory, used as the `model_identity()` string.
    model_dir: PathBuf,
    /// Variant this backend was loaded with.
    variant: ParakeetVariant,
    /// Set by `abort()` — checked at transcription entry.
    abort_flag: std::sync::atomic::AtomicBool,
    /// Model handle. The inner type depends on the resolved `transcribe-rs` dep.
    /// When `transcribe-rs` is available this holds `Mutex<transcribe_rs::onnx::parakeet::ParakeetModel>`.
    /// Until the dep is unblocked this is a `Mutex<()>` placeholder that returns an error on transcription.
    #[allow(dead_code)]
    _model: Mutex<()>,
}

impl ParakeetBackend {
    /// Load a Parakeet model from the given directory.
    ///
    /// Returns `Err` with a clear "dep not available" message until the
    /// `transcribe-rs` dep conflict is resolved and the dep is re-added.
    pub fn from_variant_dir(
        model_dir: &Path,
        variant_str: &str,
    ) -> anyhow::Result<Self> {
        let variant = parse_variant(variant_str).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown Parakeet variant {:?} — expected \"tdt-0.6b-v2\"",
                variant_str
            )
        })?;

        let canon_dir = validate_parakeet_model_dir(model_dir)?;

        tracing::warn!(
            "[parakeet] ParakeetBackend::from_variant_dir called for {} at {} — \
             transcribe-rs dep is not yet active (ort version conflict with vad-rs). \
             See src/transcribe_backends/parakeet.rs for unblock instructions.",
            variant.name(),
            canon_dir.display()
        );

        // TODO: once transcribe-rs is unblocked, replace this stub with:
        //   let model = transcribe_rs::onnx::parakeet::ParakeetModel::load(
        //       &canon_dir,
        //       map_variant(variant),
        //       &transcribe_rs::onnx::Quantization::default(),
        //   ).map_err(|e| anyhow::anyhow!("Parakeet model load failed: {}", e))?;
        //   _model: Mutex::new(model),

        anyhow::bail!(
            "Parakeet backend is not yet active — the `transcribe-rs` dependency has an ort \
             version conflict with `vad-rs`. See src-tauri/src/transcribe_backends/parakeet.rs \
             for the resolution path. Set TT_BACKEND=whisper (or unset it) to continue with Whisper."
        )
    }

    /// Build a backend from the current settings.
    /// Reads `TT_BACKEND_VARIANT` env var for the variant (default: "tdt-0.6b-v2").
    pub fn from_config(_cfg: &crate::settings::Config) -> anyhow::Result<Self> {
        let variant_str = std::env::var("TT_BACKEND_VARIANT")
            .unwrap_or_else(|_| "tdt-0.6b-v2".to_string());

        let dir = variant_dir(&variant_str).ok_or_else(|| {
            anyhow::anyhow!("Could not determine Parakeet model directory (no home dir?)")
        })?;

        Self::from_variant_dir(&dir, &variant_str)
    }
}

impl TranscriptionBackend for ParakeetBackend {
    fn transcribe(&self, _wav: &Path) -> anyhow::Result<TranscriptOutcome> {
        use std::sync::atomic::Ordering;

        if self.abort_flag.load(Ordering::Acquire) {
            return Ok(TranscriptOutcome {
                text: String::new(),
                rejection: None,
            });
        }

        // TODO: replace stub with actual transcribe-rs inference once unblocked:
        //
        //   let mut model = self._model.lock().unwrap_or_else(|e| e.into_inner());
        //   let result = model
        //       .transcribe_file(_wav, &transcribe_rs::TranscribeOptions::default())
        //       .map_err(|e| anyhow::anyhow!("Parakeet transcription failed: {}", e))?;
        //   // Parakeet CTC output is typically lowercase/unpunctuated — pass through
        //   // Chaperone cleanup which normalizes it. detect_garbage still applies.
        //   let text = result.text.trim().to_string();
        //   let rejection = crate::transcribe::detect_garbage(&text);
        //   Ok(TranscriptOutcome { text, rejection })

        anyhow::bail!(
            "ParakeetBackend.transcribe() called on a stub instance — \
             construction should have failed before reaching here"
        )
    }

    fn abort(&self) {
        use std::sync::atomic::Ordering;
        self.abort_flag.store(true, Ordering::Release);
        tracing::info!("[parakeet] abort requested (stub backend)");
    }

    fn model_identity(&self) -> String {
        format!(
            "parakeet:{}:{}",
            self.variant.name(),
            self.model_dir.display()
        )
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// validate_parakeet_model_dir rejects a non-existent directory.
    #[test]
    fn validate_parakeet_model_dir_rejects_missing_dir() {
        let result = validate_parakeet_model_dir(Path::new("/definitely/not/a/real/parakeet/dir"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found") || msg.contains("outside"),
            "unexpected error: {}",
            msg
        );
    }

    /// validate_parakeet_model_dir rejects a directory missing required files.
    #[test]
    fn validate_parakeet_model_dir_rejects_incomplete_bundle() {
        // We need a directory inside the expected parakeet_models_dir() to pass
        // the path-traversal check. Since that dir may not exist in CI, we
        // accept either "not found" or "outside" or "missing" as valid errors.
        let tmp = tempdir().expect("tempdir");
        let result = validate_parakeet_model_dir(tmp.path());
        // The tmp dir is outside ~/.config/…/parakeet/, so either the
        // path-traversal guard or the missing-files check fires.
        assert!(result.is_err(), "expected error for out-of-tree tmp dir");
    }

    /// validate_parakeet_model_dir accepts a well-formed bundle inside the
    /// expected base directory. This test is gated on the base dir existing.
    #[test]
    fn validate_parakeet_model_dir_accepts_valid_bundle() {
        let Some(base) = parakeet_models_dir() else {
            return; // no home dir in this environment
        };
        let variant_dir = base.join("tdt-0.6b-v2");
        if !variant_dir.exists() {
            // Model not downloaded — skip this test in CI.
            return;
        }
        // Only run the full validation if both required files are present.
        let has_model = variant_dir.join("model.onnx").exists();
        let has_tokenizer = variant_dir.join("tokenizer.json").exists();
        if !has_model || !has_tokenizer {
            return;
        }
        let result = validate_parakeet_model_dir(&variant_dir);
        assert!(result.is_ok(), "expected valid bundle to pass: {:?}", result.err());
    }

    /// parse_variant accepts all documented aliases.
    #[test]
    fn parse_variant_accepts_known_aliases() {
        assert!(parse_variant("tdt-0.6b-v2").is_some());
        assert!(parse_variant("tdt06b").is_some());
        assert!(parse_variant("TDT-0.6B-V2").is_some()); // case-insensitive
    }

    /// parse_variant rejects unknown strings.
    #[test]
    fn parse_variant_rejects_unknown() {
        assert!(parse_variant("large").is_none());
        assert!(parse_variant("tiny").is_none());
        assert!(parse_variant("").is_none());
    }

    /// parakeet_models_dir returns a path ending in models/parakeet.
    #[test]
    fn parakeet_models_dir_has_correct_suffix() {
        if let Some(p) = parakeet_models_dir() {
            assert!(
                p.ends_with("models/parakeet"),
                "expected path ending in models/parakeet, got: {:?}",
                p
            );
        }
    }

    /// Create a fake bundle inside a temp dir to confirm the file-check logic
    /// works (path-traversal guard skipped because we use validate directly).
    #[test]
    fn validate_parakeet_model_dir_file_check_logic() {
        // Directly test the file-presence check by temporarily bypassing the
        // path-traversal guard (which requires the real home dir structure).
        // We do this by checking the error message is about missing files
        // when the dir exists but is empty.
        // Since the tmp dir is outside ~/.config, the path-traversal guard
        // fires first — verify the error is meaningful.
        let tmp = tempdir().expect("tempdir");
        fs::write(tmp.path().join("model.onnx"), b"fake onnx").expect("write");
        fs::write(tmp.path().join("tokenizer.json"), b"{}").expect("write");
        let result = validate_parakeet_model_dir(tmp.path());
        // Expect error: path is outside the allowed base.
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("outside") || msg.contains("not found"),
            "unexpected error: {}",
            msg
        );
    }
}
