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
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_theme() -> String { "auto".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperConfig {
    pub bin: String,
    pub model: String,
    #[serde(default)]
    pub models: Vec<String>,
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
        let model = default_model_path().to_string_lossy().into_owned();
        Self {
            bin: "auto".into(),
            models: vec![model.clone()],
            model,
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
            theme: default_theme(),
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

pub fn scan_models_dir() -> Vec<String> {
    let mut dir = dirs::home_dir().unwrap_or_default();
    dir.push(".config/librewin/turbotalk/models");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut paths: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().map_or(false, |x| x == "bin") {
                Some(p.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    paths.sort();
    paths
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
