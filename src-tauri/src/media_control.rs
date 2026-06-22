// Pause/resume media playback when dictation starts/stops.
// Uses a tiny bundled helper binary that calls the private MediaRemote
// framework from its own process (safe — ObjC on its own main thread).

use std::path::PathBuf;
use std::process::Command;

fn helper_path() -> PathBuf {
    // Bundled alongside the app binary (set by Tauri's externalBin)
    let mut p = std::env::current_exe()
        .unwrap_or_default()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    p.push("media-toggle");
    p
}

fn toggle() {
    let path = helper_path();
    if !path.exists() {
        tracing::debug!("[media_control] helper not found at {:?}", path);
        return;
    }
    if let Err(e) = Command::new(&path).output() {
        tracing::warn!("[media_control] helper failed: {e}");
    }
}

/// Pause media playback. Call before dictation starts.
pub fn pause() {
    toggle();
}

/// Resume media playback. Call after dictation finishes.
pub fn resume() {
    toggle();
}
