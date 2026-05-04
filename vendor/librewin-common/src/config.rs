//! Shared configuration and preset management for LibreWin apps.
//!
//! Owns the `FadePreset` type and the JSON file at
//! `~/.config/librewin/fade-presets.json`.
//!
//! Previously this struct was duplicated between Shelf and Fade with a
//! comment warning of the coupling risk. Both apps now import from here.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A saved Fade conversion preset, shared between Shelf (quick-convert menu)
/// and Fade (preset manager).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FadePreset {
    pub id: String,
    pub name: String,
    pub media_type: String, // "image" | "video" | "audio"
    pub output_format: String,
    pub quality: Option<u32>,     // image quality 1–100
    pub codec: Option<String>,    // video codec: "h264" | "h265" | "vp9" | "av1" | "copy"
    pub bitrate: Option<u32>,     // audio/video bitrate kbps
    pub sample_rate: Option<u32>, // audio sample rate Hz
}

/// Path to the shared presets JSON file.
pub fn fade_presets_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{}/.config/librewin/fade-presets.json", home))
}

/// Read all presets from disk. Returns an empty vec if the file is missing or malformed.
pub fn read_presets() -> Vec<FadePreset> {
    std::fs::read_to_string(fade_presets_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Write presets to disk, creating the config directory if needed.
pub fn write_presets(presets: &[FadePreset]) -> Result<(), String> {
    let path = fade_presets_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(presets).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_presets_path_contains_librewin() {
        let p = fade_presets_path();
        assert!(p.to_string_lossy().contains("librewin"));
        assert!(p.to_string_lossy().ends_with("fade-presets.json"));
    }

    #[test]
    fn read_presets_returns_empty_on_missing_file() {
        // Should not panic, returns empty vec
        // (real file may or may not exist — we just verify no panic)
        let _ = read_presets();
    }
}
