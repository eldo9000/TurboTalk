// Spawns whisper-cli, feeds it the WAV, reads back the transcript.
// Output strategy: -otxt writes <wav_path>.txt; we read and delete that file.
use std::path::{Path, PathBuf};

const WHISPER_BIN: &str = "/opt/homebrew/bin/whisper-cli";

fn default_model() -> PathBuf {
    let mut p = dirs::home_dir().unwrap_or_default();
    p.push(".config/librewin/turbotalk/models/ggml-base.en.bin");
    p
}

pub fn run(wav: &Path) -> anyhow::Result<String> {
    let model = default_model();
    if !model.exists() {
        anyhow::bail!(
            "whisper model not found at {:?} — download ggml-base.en.bin into that path",
            model
        );
    }

    // whisper-cli appends .txt to the full input filename: <wav>.wav.txt
    let txt_path = PathBuf::from(format!("{}.txt", wav.display()));

    let output = std::process::Command::new(WHISPER_BIN)
        .args([
            "-m", model.to_str().unwrap(),
            "-f", wav.to_str().unwrap(),
            "-otxt",
            "-np",  // suppress diagnostics on stdout
            "-nt",  // no timestamps in text file
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
    // Capitalize first character
    let mut chars = raw.chars();
    let first = chars.next().unwrap().to_uppercase().to_string();
    first + chars.as_str()
}
