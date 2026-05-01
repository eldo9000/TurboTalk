// Spawns whisper-cli, feeds it the WAV, reads back the transcript.
// Output strategy: -otxt writes <wav_path>.wav.txt; we read and delete that file.
use std::path::{Path, PathBuf};

/// Locate the whisper-cli binary.
/// Priority: bundled sidecar (next to exe) → dev binaries dir → configured path.
fn find_whisper(configured_bin: &str) -> PathBuf {
    let sidecar = "whisper-cli-aarch64-apple-darwin";

    // Release bundle: sidecar is placed next to the main executable in Contents/MacOS/
    if let Ok(exe) = std::env::current_exe() {
        let p = exe.parent().unwrap_or_else(|| Path::new(".")).join(sidecar);
        if p.exists() {
            tracing::debug!("[transcribe] using bundled sidecar: {:?}", p);
            return p;
        }
    }

    // Dev mode: sidecar lives in src-tauri/binaries/ at compile time
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(sidecar);
    if dev.exists() {
        tracing::debug!("[transcribe] using dev sidecar: {:?}", dev);
        return dev;
    }

    // Last resort: configured path (allows Homebrew override via settings)
    tracing::debug!("[transcribe] using configured bin: {}", configured_bin);
    PathBuf::from(configured_bin)
}

pub fn run(wav: &Path) -> anyhow::Result<String> {
    let cfg = crate::settings::load();
    let bin = find_whisper(&cfg.whisper.bin);
    let model = &cfg.whisper.model;

    if !std::path::Path::new(model).exists() {
        anyhow::bail!(
            "whisper model not found at {:?} — set [whisper] model in config.toml",
            model
        );
    }

    // whisper-cli appends .txt to the full input filename: <wav>.wav.txt
    let txt_path = PathBuf::from(format!("{}.txt", wav.display()));

    let output = std::process::Command::new(&bin)
        .args([
            "-m", model,
            "-f", wav.to_str().unwrap(),
            "-otxt",
            "-np",
            "-nt",
            "-l", "en",
            "--vad",   // energy-based VAD: skip non-speech segments
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
