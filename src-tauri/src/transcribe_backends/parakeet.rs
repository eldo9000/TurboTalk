// Parakeet ONNX transcription backend
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
// ── Model storage ────────────────────────────────────────────────────────────
//
// Model files (ONNX bundle) are stored under:
//   ~/.config/turbotalk/models/parakeet/<variant>/
//
// Required files per variant (Parakeet TDT, as expected by transcribe-rs):
//   encoder-model.onnx     (the CTC/TDT encoder)
//   decoder_joint-model.onnx
//   nemo128.onnx           (mel spectrogram preprocessor)
//   vocab.txt              (sentencepiece vocab)
//
// Source: https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2
// (ONNX export — look for the ONNX community fork or the export script in the
// repo; the raw HuggingFace checkpoint is SafeTensors/PyTorch, not ONNX)
//
// ── ONNX Runtime dylib ───────────────────────────────────────────────────────
//
// Parakeet uses ONNX Runtime:
//   macOS arm64: libonnxruntime.dylib (~30 MB), declared in tauri.macos.conf.json.
// Download from: https://github.com/microsoft/onnxruntime/releases
// (match the version required by the resolved `ort` crate — currently rc.12).
//
// ── Activation ───────────────────────────────────────────────────────────────
//
// The `parakeet` Cargo feature activates this backend. It is enabled by
// default. Set TT_BACKEND=parakeet at runtime (or set backend in config)
// to route through this backend. Download a Parakeet model via
// `download_parakeet_model` before use.
//
// TODO: wire a settings UI toggle so users can pick Parakeet vs Whisper.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

use crate::transcribe::{TranscriptOutcome, TranscriptionBackend};

// ── Variant type ─────────────────────────────────────────────────────────────
//
// We define our own variant enum so the enum is available for path helpers
// independently of the transcribe-rs import.

/// Parakeet model variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParakeetVariant {
    /// Parakeet TDT 0.6B v2 — English-only, fastest path for monolingual dictation.
    Tdt06bV2,
    /// Parakeet TDT 0.6B v3 — multilingual (25 European languages), same ONNX layout as v2.
    Tdt06bV3,
}

impl ParakeetVariant {
    /// Directory-safe name for the variant (used as storage subdirectory).
    pub fn name(self) -> &'static str {
        match self {
            ParakeetVariant::Tdt06bV2 => "tdt-0.6b-v2",
            ParakeetVariant::Tdt06bV3 => "tdt-0.6b-v3",
        }
    }

    /// Human-readable display name.
    pub fn display_name(self) -> &'static str {
        match self {
            ParakeetVariant::Tdt06bV2 => "Parakeet TDT 0.6B v2",
            ParakeetVariant::Tdt06bV3 => "Parakeet TDT 0.6B v3",
        }
    }
}

/// Parse a user-facing variant string (case-insensitive).
/// Returns `None` for unrecognised strings.
pub fn parse_variant(s: &str) -> Option<ParakeetVariant> {
    match s.to_lowercase().as_str() {
        "tdt-0.6b-v2" | "tdt06b" | "tdt_0.6b_v2" => Some(ParakeetVariant::Tdt06bV2),
        "tdt-0.6b-v3" | "tdt06b_v3" | "tdt_0.6b_v3" => Some(ParakeetVariant::Tdt06bV3),
        _ => None,
    }
}

// ── Model path helpers ────────────────────────────────────────────────────────

/// Base directory for Parakeet model bundles.
/// Uses `data_dir()` so this path matches `scan_models_dir` and Whisper model
/// storage (all under `data_dir().join("models/")`).
///
/// Returns `~/Library/Application Support/turbotalk/models/parakeet/` on macOS.
///
/// Does NOT require the directory to exist — callers that need it to exist
/// (e.g. the download command) create it themselves.
pub fn parakeet_models_dir() -> Option<PathBuf> {
    Some(crate::settings::data_dir().join("models/parakeet"))
}

/// Path to the ONNX bundle directory for a specific variant.
/// Returns `~/.config/turbotalk/models/parakeet/<variant>/`.
pub fn variant_dir(variant_name_str: &str) -> Option<PathBuf> {
    let mut p = parakeet_models_dir()?;
    p.push(variant_name_str);
    Some(p)
}

/// Validate that a Parakeet model directory exists and contains the required
/// files as expected by `transcribe-rs`:
///   `encoder-model.onnx`, `decoder_joint-model.onnx`, `nemo128.onnx`, `vocab.txt`.
///
/// Returns `Ok(canonicalized_path)` on success.
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

    // Check required files (int8 preferred; fp32 accepted as fallback).
    let has_encoder =
        canon.join("encoder-model.int8.onnx").exists() || canon.join("encoder-model.onnx").exists();
    let has_decoder = canon.join("decoder_joint-model.int8.onnx").exists()
        || canon.join("decoder_joint-model.onnx").exists();
    if !has_encoder {
        anyhow::bail!(
            "Parakeet model incomplete — missing encoder-model(.int8).onnx in {}. \
             Re-run download_parakeet_model.",
            canon.display()
        );
    }
    if !has_decoder {
        anyhow::bail!(
            "Parakeet model incomplete — missing decoder_joint-model(.int8).onnx in {}. \
             Re-run download_parakeet_model.",
            canon.display()
        );
    }
    for filename in &["nemo128.onnx", "vocab.txt"] {
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

/// Peak target for Parakeet input — matches `audio.rs` NORMALIZE_PEAK.
const PARAKEET_TARGET_PEAK: f32 = 0.89;

/// Read a 16 kHz mono WAV and re-boost quiet peaks before inference.
fn prepare_parakeet_samples(wav: &Path) -> anyhow::Result<Vec<f32>> {
    let mut samples = transcribe_rs::audio::read_wav_samples(wav)
        .map_err(|e| anyhow::anyhow!("read wav {}: {e}", wav.display()))?;
    let peak = samples.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
    if peak > 0.0 && peak < PARAKEET_TARGET_PEAK {
        let gain = PARAKEET_TARGET_PEAK / peak;
        for s in &mut samples {
            *s *= gain;
        }
    }
    tracing::info!(
        "[parakeet] input {} samples ({:.2}s) peak={:.4}",
        samples.len(),
        samples.len() as f32 / 16_000.0,
        peak
    );
    Ok(samples)
}

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
/// interrupt an in-flight ONNX session. Recorder cancellation moves the job
/// back to Ready; the old transcription finishes naturally and is discarded by
/// the hotkey lifecycle because the recorder state no longer permits Cleaning.
pub struct ParakeetBackend {
    /// Canonicalized model directory, used as the `model_identity()` string.
    model_dir: PathBuf,
    /// Variant this backend was loaded with.
    variant: ParakeetVariant,
    /// Loaded Parakeet model wrapped in a Mutex for Sync.
    model: Mutex<ParakeetModel>,
}

impl ParakeetBackend {
    /// Load a Parakeet model from the given directory.
    pub fn from_variant_dir(model_dir: &Path, variant_str: &str) -> anyhow::Result<Self> {
        let variant = parse_variant(variant_str).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown Parakeet variant {:?} — expected \"tdt-0.6b-v2\" or \"tdt-0.6b-v3\"",
                variant_str
            )
        })?;

        let canon_dir = validate_parakeet_model_dir(model_dir)?;

        let quantization = if canon_dir.join("encoder-model.int8.onnx").exists() {
            Quantization::Int8
        } else {
            Quantization::default()
        };

        tracing::info!(
            "[parakeet] loading {} model from {} (quant={:?})",
            variant.display_name(),
            canon_dir.display(),
            quantization
        );

        // Note: ParakeetModel::load does NOT take a variant parameter — the
        // variant is implicit in the model files present in the directory.
        let model = ParakeetModel::load(&canon_dir, &quantization)
            .map_err(|e| anyhow::anyhow!("Parakeet model load failed: {e}"))?;

        tracing::info!("[parakeet] model loaded successfully");

        Ok(Self {
            model_dir: canon_dir,
            variant,
            model: Mutex::new(model),
        })
    }

    /// Build a backend from the current settings.
    pub fn from_config(cfg: &crate::settings::Config) -> anyhow::Result<Self> {
        let variant_str = crate::settings::resolve_backend_variant(cfg);

        let dir = variant_dir(&variant_str).ok_or_else(|| {
            anyhow::anyhow!("Could not determine Parakeet model directory (no home dir?)")
        })?;

        Self::from_variant_dir(&dir, &variant_str)
    }
}

impl TranscriptionBackend for ParakeetBackend {
    fn transcribe(&self, wav: &Path) -> anyhow::Result<TranscriptOutcome> {
        let mut model = self.model.lock().unwrap_or_else(|e| e.into_inner());

        let samples = prepare_parakeet_samples(wav)?;
        let params = ParakeetParams::default();

        let result = model
            .transcribe_with(&samples, &params)
            .map_err(|e| anyhow::anyhow!("Parakeet transcription failed: {e}"))?;

        // Parakeet is CTC/TDT — structural guarantee: no hallucination on
        // silence (unlike autoregressive models). The only artifact is syllable
        // fragment repetition ("war war war warm-up") from ambiguous CTC
        // emissions in low-signal regions. Check only that — the other 4
        // garbage signals apply to Whisper's failure modes only.
        let text = crate::transcribe::normalize_whisper_text(result.text.trim());
        let text = crate::transcribe::strip_trailing_filler(&text);
        let rejection = crate::transcribe::detect_prefix_fragment(&text);

        tracing::info!(
            "[parakeet] transcribed ({} chars, rejection={:?})",
            text.chars().count(),
            rejection
        );
        crate::diagnostic_log::record_transcript("parakeet", &text, &format!("{rejection:?}"));

        Ok(TranscriptOutcome { text, rejection })
    }

    fn abort(&self) {
        tracing::info!("[parakeet] abort requested — in-process inference will finish naturally");
    }

    fn invalidate_after_abort(&self) -> bool {
        false
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
        // Only run the full validation if all required files are present.
        let has_encoder = variant_dir.join("encoder-model.onnx").exists();
        let has_decoder = variant_dir.join("decoder_joint-model.onnx").exists();
        let has_nemo = variant_dir.join("nemo128.onnx").exists();
        let has_vocab = variant_dir.join("vocab.txt").exists();
        if !has_encoder || !has_decoder || !has_nemo || !has_vocab {
            return;
        }
        let result = validate_parakeet_model_dir(&variant_dir);
        assert!(
            result.is_ok(),
            "expected valid bundle to pass: {:?}",
            result.err()
        );
    }

    /// parse_variant accepts all documented aliases.
    #[test]
    fn parse_variant_accepts_known_aliases() {
        assert!(parse_variant("tdt-0.6b-v2").is_some());
        assert!(parse_variant("tdt-0.6b-v3").is_some());
        assert!(parse_variant("tdt06b").is_some());
        assert!(parse_variant("TDT-0.6B-V3").is_some()); // case-insensitive
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

    /// A tmp dir outside the expected base must always be rejected — either by
    /// the path-traversal guard (when the base dir exists) or by the missing-
    /// files check (when the base dir doesn't exist yet). The test asserts that
    /// an error is always returned for an out-of-tree directory regardless of
    /// which guard fires.
    #[test]
    fn validate_parakeet_model_dir_file_check_logic() {
        // We do NOT write all required files here — the intent is to confirm
        // that an out-of-tree dir is always rejected. The path-traversal guard
        // fires when ~/.config/…/parakeet exists; otherwise the missing-files
        // check catches it (since the dir is empty).
        let tmp = tempdir().expect("tempdir");
        // Write only some files — so even if path-traversal is skipped (base
        // dir doesn't exist yet), the missing-files check fires.
        fs::write(tmp.path().join("encoder-model.onnx"), b"fake onnx").expect("write");
        let result = validate_parakeet_model_dir(tmp.path());
        assert!(result.is_err(), "out-of-tree dir must always be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("outside") || msg.contains("not found") || msg.contains("missing"),
            "unexpected error: {}",
            msg
        );
    }
}
