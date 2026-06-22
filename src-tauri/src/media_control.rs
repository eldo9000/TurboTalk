// Pause/resume media playback when dictation starts/stops.
// Uses osascript playpause on already-running media apps (pgrep gate
// prevents launching). Fast, safe, no permissions needed.

use std::sync::atomic::{AtomicBool, Ordering};

static WAS_PLAYING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
fn app_running(name: &str) -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", name])
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn osascript_playpause(app: &str) {
    let _ = std::process::Command::new("osascript")
        .args(["-e", &format!(r#"tell application "{}" to playpause"#, app)])
        .output();
}

#[cfg(target_os = "macos")]
fn toggle_running_media() {
    for app in &["Music", "Spotify"] {
        if app_running(app) {
            osascript_playpause(app);
        }
    }
}

#[cfg(target_os = "macos")]
fn any_playing() -> bool {
    for app in &["Music", "Spotify"] {
        if !app_running(app) {
            continue;
        }
        let out = std::process::Command::new("osascript")
            .args(["-e", &format!(r#"tell application "{}" to get player state"#, app)])
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
    #[cfg(target_os = "macos")]
    {
        if !any_playing() {
            WAS_PLAYING.store(false, Ordering::Release);
            return;
        }
        toggle_running_media();
        WAS_PLAYING.store(true, Ordering::Release);
    }
}

/// Resume media playback. Call after dictation finishes.
/// Only toggles if we paused something earlier.
pub fn resume() {
    #[cfg(target_os = "macos")]
    {
        if !WAS_PLAYING.load(Ordering::Acquire) {
            return;
        }
        // Let audio quality settle after recording before resuming.
        std::thread::sleep(std::time::Duration::from_millis(500));
        toggle_running_media();
        WAS_PLAYING.store(false, Ordering::Release);
    }
}
