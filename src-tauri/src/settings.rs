// Config persistence — ~/.config/librewin/turbotalk/config.toml
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub whisper: WhisperConfig,
    #[serde(default)]
    pub cleanup: CleanupConfig,
    #[serde(default)]
    pub audio: AudioConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperConfig {
    pub bin: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupConfig {
    /// "off" | "regex" | "chaperone"
    pub mode: String,
    pub ollama_url: String,
    pub classifier_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub device: String,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            bin: "/opt/homebrew/bin/whisper-cli".into(),
            model: default_model_path()
                .to_string_lossy()
                .into_owned(),
        }
    }
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            mode: "regex".into(),
            ollama_url: "http://localhost:11434".into(),
            classifier_model: "llama3.2:3b".into(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            device: "default".into(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            whisper: WhisperConfig::default(),
            cleanup: CleanupConfig::default(),
            audio: AudioConfig::default(),
        }
    }
}

fn config_path() -> PathBuf {
    let mut p = dirs::home_dir().unwrap_or_default();
    p.push(".config/librewin/turbotalk/config.toml");
    p
}

fn default_model_path() -> PathBuf {
    let mut p = dirs::home_dir().unwrap_or_default();
    p.push(".config/librewin/turbotalk/models/ggml-base.en.bin");
    p
}

pub fn load() -> Config {
    let path = config_path();
    if let Ok(contents) = std::fs::read_to_string(&path) {
        match toml::from_str::<Config>(&contents) {
            Ok(cfg) => return cfg,
            Err(e) => tracing::warn!("[settings] parse error, using defaults: {:?}", e),
        }
    }
    Config::default()
}

pub fn save(cfg: &Config) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = toml::to_string_pretty(cfg)?;
    std::fs::write(&path, contents)?;
    Ok(())
}
