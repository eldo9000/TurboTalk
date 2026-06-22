// Pause/resume media playback when dictation starts/stops.
// Uses a helper binary that posts the same NXSYSDEFINED media key
// event as the physical Play/Pause key on an Apple keyboard.

use std::path::PathBuf;
use std::process::Command;

static WAS_PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn helper_path() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
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
        None => return,
    };
    let _ = Command::new(&path).output();
}

/// Check if a media app is currently playing. Only queries running apps
/// to avoid launching anything.
#[cfg(target_os = "macos")]
fn is_playing() -> bool {
    for app in &["Music", "Spotify"] {
        let out = Command::new("osascript")
            .args(["-e", &format!(
                r#"tell application "System Events" to if exists (process "{}") then tell application "{}" to get player state"#,
                app, app
            )])
            .output();
        if let Ok(out) = out {
            if let Ok(s) = String::from_utf8(out.stdout) {
                if s.trim() == "playing" {
                    return true;
                }
            }
        }
    }
    false
}

/// Pause media playback. Call before dictation starts.
/// Only toggles if something is actually playing.
pub fn pause() {
    if !is_playing() {
        tracing::debug!("[media_control] nothing playing, skipping pause");
        WAS_PLAYING.store(false, std::sync::atomic::Ordering::Release);
        return;
    }
    tracing::debug!("[media_control] pausing playback");
    toggle();
    WAS_PLAYING.store(true, std::sync::atomic::Ordering::Release);
}

/// Resume media playback. Call after dictation finishes.
/// Only toggles if we paused something earlier.
pub fn resume() {
    if !WAS_PLAYING.load(std::sync::atomic::Ordering::Acquire) {
        tracing::debug!("[media_control] nothing was paused, skipping resume");
        return;
    }
    // Let audio quality settle after recording before resuming playback.
    std::thread::sleep(std::time::Duration::from_millis(500));
    tracing::debug!("[media_control] resuming playback");
    toggle();
    WAS_PLAYING.store(false, std::sync::atomic::Ordering::Release);
}

#[cfg(not(target_os = "macos"))]
fn is_playing() -> bool { false }
