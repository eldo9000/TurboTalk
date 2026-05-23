// Moonshine ONNX transcription backend (TASK-58)
//
// Implements `TranscriptionBackend` using the `transcribe-rs` crate's Moonshine
// support. Moonshine is a non-autoregressive ONNX speech recognition model
// designed for dictation — it does not hallucinate "thanks for watching" on
// silence the way Whisper does, making it a structural fix for what TASK-55/56
// patched around.
//
// ── Model storage ────────────────────────────────────────────────────────────
//
// Model files (ONNX bundle + tokenizer) are stored under:
//   ~/.config/librewin/turbotalk/models/moonshine/<variant>/
//
// Required files per variant:
//   encoder_model.onnx
//   decoder_model_merged.onnx
//   tokenizer.json
//
// Source URLs (HuggingFace ONNX community):
//   tiny: https://huggingface.co/onnx-community/moonshine-tiny-ONNX
//   base: https://huggingface.co/onnx-community/moonshine-base-ONNX
//
// ── ONNX Runtime dylib ───────────────────────────────────────────────────────
//
// macOS arm64: libonnxruntime.dylib (~30 MB).
// Must be placed in src-tauri/binaries/ and is declared in tauri.macos.conf.json.
// Download from: https://github.com/microsoft/onnxruntime/releases
// (match the version required by the resolved `ort` crate — currently rc.12).
//
// ── Activation ───────────────────────────────────────────────────────────────
//
// The `moonshine` Cargo feature activates this backend. It is enabled by
// default. Set TT_BACKEND=moonshine at runtime (or set backend in config)
// to route through this backend. Download a Moonshine model via
// `download_moonshine_model` before use.
//
// TODO (TASK-60): wire a settings UI toggle so users can pick Moonshine vs Whisper.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use transcribe_rs::onnx::moonshine::{MoonshineModel, MoonshineVariant as TrsMoonshineVariant};
use transcribe_rs::onnx::Quantization;
use transcribe_rs::{SpeechModel, TranscribeOptions};

use crate::transcribe::{TranscriptOutcome, TranscriptionBackend};

// ── Variant type ─────────────────────────────────────────────────────────────
//
// We define our own variant enum so the enum is available for path helpers
// independently of the transcribe-rs import.

/// Moonshine model size variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoonshineVariant {
    /// ~38 MB ONNX; 6-layer decoder. Fast, good enough for most dictation.
    Tiny,
    /// ~65 MB ONNX; 8-layer decoder. Better accuracy, still real-time on M-series.
    Base,
}

impl MoonshineVariant {
    /// Human-readable name for the variant.
    pub fn name(self) -> &'static str {
        match self {
            MoonshineVariant::Tiny => "tiny",
            MoonshineVariant::Base => "base",
        }
    }
}

/// Map our variant enum to the transcribe-rs variant enum.
fn to_trs_variant(v: MoonshineVariant) -> TrsMoonshineVariant {
    match v {
        MoonshineVariant::Tiny => TrsMoonshineVariant::Tiny,
        MoonshineVariant::Base => TrsMoonshineVariant::Base,
    }
}

/// Parse a user-facing variant string (case-insensitive).
/// Returns `None` for unrecognised strings.
pub fn parse_variant(s: &str) -> Option<MoonshineVariant> {
    match s.to_lowercase().as_str() {
        "tiny" => Some(MoonshineVariant::Tiny),
        "base" => Some(MoonshineVariant::Base),
        _ => None,
    }
}

// ── Model path helpers ────────────────────────────────────────────────────────

/// Base directory for Moonshine model bundles.
/// Returns `~/.config/librewin/turbotalk/models/moonshine/`.
///
/// Does NOT require the directory to exist — callers that need it to exist
/// (e.g. the download command) create it themselves.
pub fn moonshine_models_dir() -> Option<PathBuf> {
    let mut p = dirs::home_dir()?;
    p.push(".config/librewin/turbotalk/models/moonshine");
    Some(p)
}

/// Path to the ONNX bundle directory for a specific variant.
/// Returns `~/.config/librewin/turbotalk/models/moonshine/<variant>/`.
pub fn variant_dir(variant_name_str: &str) -> Option<PathBuf> {
    let mut p = moonshine_models_dir()?;
    p.push(variant_name_str);
    Some(p)
}

/// Validate that a Moonshine model directory exists and contains the three
/// required files: `encoder_model.onnx`, `decoder_model_merged.onnx`,
/// `tokenizer.json`. Returns `Ok(canonicalized_path)` on success.
///
/// Mirrors `validate_model_path` in `transcribe.rs` but for a directory
/// rather than a single `.bin` file.
pub fn validate_moonshine_model_dir(dir: &Path) -> anyhow::Result<PathBuf> {
    let canon = dir.canonicalize().map_err(|_| {
        anyhow::anyhow!(
            "Moonshine model directory not found: {}. \
             Run download_moonshine_model to fetch it.",
            dir.display()
        )
    })?;

    // Verify the directory is inside the expected base to block path traversal.
    if let Some(base) = moonshine_models_dir() {
        if let Ok(canon_base) = base.canonicalize() {
            if !canon.starts_with(&canon_base) {
                anyhow::bail!(
                    "Moonshine model directory is outside the allowed path: {}",
                    dir.display()
                );
            }
        }
    }

    // Check required files.
    for filename in &["encoder_model.onnx", "decoder_model_merged.onnx", "tokenizer.json"] {
        let f = canon.join(filename);
        if !f.exists() {
            anyhow::bail!(
                "Moonshine model incomplete — missing {filename} in {}. \
                 Re-run download_moonshine_model.",
                canon.display()
            );
        }
    }

    Ok(canon)
}

// ── MoonshineBackend ──────────────────────────────────────────────────────────

/// `TranscriptionBackend` backed by Moonshine ONNX via `transcribe-rs`.
///
/// # Thread safety
///
/// `MoonshineModel` (from transcribe-rs) is not `Sync` (mutable inference
/// state). We wrap it in a `Mutex` so `MoonshineBackend` can be
/// `Send + Sync` as required by `Arc<dyn TranscriptionBackend>`.
/// Transcription is serialized through the mutex, matching the one-in-flight
/// invariant already enforced by WhisperBackend's `spawn_lock`.
///
/// # Abort
///
/// Moonshine runs entirely in-process (no subprocess), so `abort()` cannot
/// interrupt an in-flight ONNX session. The `abort_flag` is set and checked
/// at the transcription entry point — a cancel takes effect between recordings
/// but not mid-inference. In-flight inference completes naturally (<1 s for Tiny/Base).
pub struct MoonshineBackend {
    /// Canonicalized model directory, used as the `model_identity()` string.
    model_dir: PathBuf,
    /// Variant this backend was loaded with.
    variant: MoonshineVariant,
    /// Set by `abort()` — checked at transcription entry.
    abort_flag: std::sync::atomic::AtomicBool,
    /// Loaded Moonshine model wrapped in a Mutex for Sync.
    model: Mutex<MoonshineModel>,
}

impl MoonshineBackend {
    /// Load a Moonshine model from the given directory.
    pub fn from_variant_dir(
        model_dir: &Path,
        variant_str: &str,
    ) -> anyhow::Result<Self> {
        let variant = parse_variant(variant_str).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown Moonshine variant {:?} — expected \"tiny\" or \"base\"",
                variant_str
            )
        })?;

        let canon_dir = validate_moonshine_model_dir(model_dir)?;

        tracing::info!(
            "[moonshine] loading {} model from {}",
            variant.name(),
            canon_dir.display()
        );

        let model = MoonshineModel::load(
            &canon_dir,
            to_trs_variant(variant),
            &Quantization::default(),
        )
        .map_err(|e| anyhow::anyhow!("Moonshine model load failed: {e}"))?;

        tracing::info!("[moonshine] model loaded successfully");

        Ok(Self {
            model_dir: canon_dir,
            variant,
            abort_flag: std::sync::atomic::AtomicBool::new(false),
            model: Mutex::new(model),
        })
    }

    /// Build a backend from the current settings.
    /// Reads `TT_BACKEND_VARIANT` env var for the variant (default: "base").
    pub fn from_config(_cfg: &crate::settings::Config) -> anyhow::Result<Self> {
        let variant_str = std::env::var("TT_BACKEND_VARIANT")
            .unwrap_or_else(|_| "base".to_string());

        let dir = variant_dir(&variant_str).ok_or_else(|| {
            anyhow::anyhow!("Could not determine Moonshine model directory (no home dir?)")
        })?;

        Self::from_variant_dir(&dir, &variant_str)
    }
}

impl TranscriptionBackend for MoonshineBackend {
    fn transcribe(&self, wav: &Path) -> anyhow::Result<TranscriptOutcome> {
        use std::sync::atomic::Ordering;

        if self.abort_flag.load(Ordering::Acquire) {
            return Ok(TranscriptOutcome {
                text: String::new(),
                rejection: None,
            });
        }

        let mut model = self.model.lock().unwrap_or_else(|e| e.into_inner());

        let result = model
            .transcribe_file(wav, &TranscribeOptions::default())
            .map_err(|e| anyhow::anyhow!("Moonshine transcription failed: {e}"))?;

        let text = result.text.trim().to_string();
        let rejection = crate::transcribe::detect_garbage(&text);

        tracing::info!(
            "[moonshine] transcribed: {:?} (rejection={:?})",
            text,
            rejection
        );

        Ok(TranscriptOutcome { text, rejection })
    }

    fn abort(&self) {
        use std::sync::atomic::Ordering;
        self.abort_flag.store(true, Ordering::Release);
        tracing::info!("[moonshine] abort requested");
    }

    fn model_identity(&self) -> String {
        format!(
            "moonshine:{}:{}",
            self.variant.name(),
            self.model_dir.display()
        )
    }
}
