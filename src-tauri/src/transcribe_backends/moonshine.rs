// Moonshine ONNX transcription backend (TASK-58)
//
// Implements `TranscriptionBackend` using the `transcribe-rs` crate's Moonshine
// support. Moonshine is a non-autoregressive ONNX speech recognition model
// designed for dictation — it does not hallucinate "thanks for watching" on
// silence the way Whisper does, making it a structural fix for what TASK-55/56
// patched around.
//
// ── DEPENDENCY CONFLICT (currently unresolved) ───────────────────────────────
//
// `transcribe-rs 0.3.11` (needed for Moonshine) pins `ort = "=2.0.0-rc.12"`.
// `vad-rs 0.1.5`          (used for in-process VAD) pins `ort = "=2.0.0-rc.9"`.
// Cargo cannot resolve two exact-pinned versions of the same crate, even when
// one is declared optional. This means `transcribe-rs` cannot be added to
// Cargo.toml at all until one of these unblocking paths is taken:
//
//   (a) vad-rs upgrades to ort rc.12 — simplest, wait for upstream.
//   (b) Replace vad-rs with a direct ort rc.12 VAD integration.
//   (c) Move transcribe-rs into a separate Cargo workspace member / sidecar.
//
// Until unblocked, this file is guarded by `#[cfg(feature = "moonshine")]` so
// it does not affect the default build. The `moonshine` feature is declared in
// Cargo.toml but has no dep attached yet (see the comment there).
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
// Set TT_BACKEND=moonshine at runtime to route through this backend.
// Once the dep conflict is resolved:
//   1. Uncomment `transcribe-rs` in Cargo.toml.
//   2. Build with `cargo build --features moonshine`.
//   3. Set TT_BACKEND=moonshine, download a Moonshine model via
//      `download_moonshine_model`, and hold PTT to test.
//
// TODO (TASK-60): wire a settings UI toggle so users can pick Moonshine vs Whisper.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::transcribe::{TranscriptOutcome, TranscriptionBackend};

// ── Variant type ─────────────────────────────────────────────────────────────
//
// We define our own variant enum rather than re-exporting from transcribe-rs
// so the enum is available for path helpers even before the dep is unblocked.

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
///
/// # Build status
///
/// This struct is present in the module tree but requires the `moonshine`
/// feature AND the `transcribe-rs` dep to be active. Until the ort version
/// conflict is resolved, the `moonshine` feature compiles a stub that
/// panics at construction time with a clear error message.
pub struct MoonshineBackend {
    /// Canonicalized model directory, used as the `model_identity()` string.
    model_dir: PathBuf,
    /// Variant this backend was loaded with.
    variant: MoonshineVariant,
    /// Set by `abort()` — checked at transcription entry.
    abort_flag: std::sync::atomic::AtomicBool,
    /// Model handle. The inner type depends on the resolved `transcribe-rs` dep.
    /// When `transcribe-rs` is available this holds `Mutex<transcribe_rs::onnx::moonshine::MoonshineModel>`.
    /// Until the dep is unblocked this is a `Mutex<()>` placeholder that returns an error on transcription.
    #[allow(dead_code)]
    _model: Mutex<()>,
}

impl MoonshineBackend {
    /// Load a Moonshine model from the given directory.
    ///
    /// Returns `Err` with a clear "dep not available" message until the
    /// `transcribe-rs` dep conflict is resolved and the dep is re-added.
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

        tracing::warn!(
            "[moonshine] MoonshineBackend::from_variant_dir called for {} at {} — \
             transcribe-rs dep is not yet active (ort version conflict with vad-rs). \
             See src/transcribe_backends/moonshine.rs for unblock instructions.",
            variant.name(),
            canon_dir.display()
        );

        // TODO: once transcribe-rs is unblocked, replace this stub with:
        //   let model = transcribe_rs::onnx::moonshine::MoonshineModel::load(
        //       &canon_dir,
        //       map_variant(variant),
        //       &transcribe_rs::onnx::Quantization::default(),
        //   ).map_err(|e| anyhow::anyhow!("Moonshine model load failed: {}", e))?;
        //   _model: Mutex::new(model),

        anyhow::bail!(
            "Moonshine backend is not yet active — the `transcribe-rs` dependency has an ort \
             version conflict with `vad-rs`. See src-tauri/src/transcribe_backends/moonshine.rs \
             for the resolution path. Set TT_BACKEND=whisper (or unset it) to continue with Whisper."
        )
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
        //       .map_err(|e| anyhow::anyhow!("Moonshine transcription failed: {}", e))?;
        //   let text = result.text.trim().to_string();
        //   let rejection = crate::transcribe::detect_garbage(&text);
        //   Ok(TranscriptOutcome { text, rejection })

        anyhow::bail!(
            "MoonshineBackend.transcribe() called on a stub instance — \
             construction should have failed before reaching here"
        )
    }

    fn abort(&self) {
        use std::sync::atomic::Ordering;
        self.abort_flag.store(true, Ordering::Release);
        tracing::info!("[moonshine] abort requested (stub backend)");
    }

    fn model_identity(&self) -> String {
        format!(
            "moonshine:{}:{}",
            self.variant.name(),
            self.model_dir.display()
        )
    }
}
