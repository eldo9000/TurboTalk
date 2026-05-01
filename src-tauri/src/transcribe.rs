// Spawns whisper-cli, feeds it the WAV, reads back the transcript.
// Output strategy: -otxt writes <wav_path>.wav.txt; we read and delete that file.
use std::path::{Path, PathBuf};

/// Allowed roots for the whisper-cli binary:
/// - the directory containing the running executable (release bundle sidecar)
/// - the cargo target/ tree of this crate (dev builds)
/// - the `src-tauri/binaries/` directory bundled at compile time (dev fallback)
///
/// Any configured path that does not canonicalize to a location inside one of
/// these roots is rejected — including arbitrary system binaries like `/bin/ls`.
fn allowed_whisper_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if let Ok(canon) = parent.canonicalize() {
                roots.push(canon);
            }
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Ok(canon) = manifest_dir.join("target").canonicalize() {
        roots.push(canon);
    }
    if let Ok(canon) = manifest_dir.join("binaries").canonicalize() {
        roots.push(canon);
    }

    roots
}

/// Returns `true` only if `p` canonicalizes to a path inside one of the
/// allowed whisper roots. Symlinks are resolved before checking.
fn is_allowed_whisper_path(p: &Path) -> bool {
    let Ok(canon) = p.canonicalize() else {
        return false;
    };
    let roots = allowed_whisper_roots();
    roots.iter().any(|root| canon.starts_with(root))
}

/// Locate the whisper-cli binary.
/// Priority: bundled sidecar (next to exe) → dev binaries dir → configured path.
/// The configured-path fallback is only honored if the path canonicalizes to
/// a location inside an allowed root; otherwise an error is returned.
fn find_whisper(configured_bin: &str) -> anyhow::Result<PathBuf> {
    let sidecar = "whisper-cli-aarch64-apple-darwin";

    // Release bundle: sidecar is placed next to the main executable in Contents/MacOS/
    if let Ok(exe) = std::env::current_exe() {
        let p = exe.parent().unwrap_or_else(|| Path::new(".")).join(sidecar);
        if p.exists() {
            tracing::debug!("[transcribe] using bundled sidecar: {:?}", p);
            return Ok(p);
        }
    }

    // Dev mode: sidecar lives in src-tauri/binaries/ at compile time
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(sidecar);
    if dev.exists() {
        tracing::debug!("[transcribe] using dev sidecar: {:?}", dev);
        return Ok(dev);
    }

    // Last resort: configured path. Validated against the allow-list to prevent
    // arbitrary code execution via a tampered config.toml.
    let configured = PathBuf::from(configured_bin);
    if !is_allowed_whisper_path(&configured) {
        tracing::error!(
            "[transcribe] rejected configured whisper bin (outside allowed roots): {}",
            configured_bin
        );
        anyhow::bail!(
            "configured whisper.bin is outside allowed locations: {} — \
             remove it from config.toml so the bundled sidecar is used",
            configured_bin
        );
    }
    tracing::debug!("[transcribe] using configured bin: {}", configured_bin);
    Ok(configured)
}

pub fn run(wav: &Path) -> anyhow::Result<String> {
    let cfg = crate::settings::load();
    let bin = find_whisper(&cfg.whisper.bin)?;

    // Canonicalize the model path and verify it lives inside the allowed
    // models directory. Blocks `model = "/etc/passwd"` style attacks and
    // symlink escapes from the models dir.
    let raw_model = &cfg.whisper.model;
    let canon_model = PathBuf::from(raw_model).canonicalize().map_err(|_| {
        anyhow::anyhow!(
            "model path does not exist or could not be resolved: {}",
            raw_model
        )
    })?;
    let canon_models_dir = crate::settings::canonical_models_dir().ok_or_else(|| {
        anyhow::anyhow!(
            "models directory does not exist — create ~/.config/librewin/turbotalk/models/ \
             and place a ggml model there"
        )
    })?;
    if !canon_model.starts_with(&canon_models_dir) {
        anyhow::bail!(
            "model path is outside the allowed models directory: {}",
            raw_model
        );
    }
    let model_str = canon_model
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("model path is not valid UTF-8: {:?}", canon_model))?;

    // whisper-cli appends .txt to the full input filename: <wav>.wav.txt
    let txt_path = PathBuf::from(format!("{}.txt", wav.display()));

    let output = std::process::Command::new(&bin)
        .args([
            "-m", model_str,
            "-f", wav.to_str().unwrap(),
            "-otxt",
            "-np",
            "-nt",
            "-l", "en",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("whisper-cli failed ({}): {}", output.status, stderr);
    }

    let text = std::fs::read_to_string(&txt_path)
        .map_err(|_| anyhow::anyhow!("whisper output file not found: {:?}", txt_path))?;
    let _ = std::fs::remove_file(&txt_path);

    Ok(crate::cleanup::process(text.trim()))
}
