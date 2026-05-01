// Spawns whisper-cli, feeds it the WAV, reads back the transcript.
// Output strategy: -otxt writes <wav_path>.wav.txt; we read and delete that file.
use std::path::{Path, PathBuf};

pub fn run(wav: &Path) -> anyhow::Result<String> {
    let cfg = crate::settings::load();
    let bin = &cfg.whisper.bin;
    let model = &cfg.whisper.model;

    if !std::path::Path::new(model).exists() {
        anyhow::bail!(
            "whisper model not found at {:?} — set [whisper] model in config.toml",
            model
        );
    }

    // whisper-cli appends .txt to the full input filename: <wav>.wav.txt
    let txt_path = PathBuf::from(format!("{}.txt", wav.display()));

    let output = std::process::Command::new(bin)
        .args([
            "-m", model,
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

    Ok(cleanup(text.trim()))
}

fn cleanup(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let mut chars = raw.chars();
    let first = chars.next().unwrap().to_uppercase().to_string();
    first + chars.as_str()
}
