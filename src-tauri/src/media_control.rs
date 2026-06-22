// Pause/resume media playback when dictation starts/stops.
// Uses a helper binary that posts the same NXSYSDEFINED media key
// event as the physical Play/Pause key on an Apple keyboard.

use std::path::PathBuf;
use std::process::Command;

fn helper_path() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    // Try target-triple name (bundled), then bare name (dev)
    for name in &["media-toggle-aarch64-apple-darwin", "media-toggle"] {
        let p = exe_dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn toggle() {
    let path = match helper_path() {
        Some(p) => p,
        None => {
            tracing::debug!("[media_control] helper not found");
            return;
        }
    };
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
