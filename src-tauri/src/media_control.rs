// Pause/resume media playback when dictation starts/stops.
// Uses osascript playpause on running media apps.

use std::sync::atomic::{AtomicBool, Ordering};

static WAS_PLAYING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
fn toggle(app: &str) {
    let _ = std::process::Command::new("osascript")
        .args(["-e", &format!(r#"tell application "{}" to playpause"#, app)])
        .output();
}

#[cfg(target_os = "macos")]
fn is_playing() -> bool {
    for app in &["Music", "Spotify"] {
        let out = std::process::Command::new("osascript")
            .args(["-e", &format!(r#"if application "{}" is running then tell application "{}" to get player state"#, app, app)])
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
pub fn pause() {
    #[cfg(target_os = "macos")]
    {
        if !is_playing() {
            WAS_PLAYING.store(false, Ordering::Release);
            return;
        }
        toggle("Music");
        toggle("Spotify");
        WAS_PLAYING.store(true, Ordering::Release);
    }
}

/// Resume media playback. Call after dictation finishes.
pub fn resume() {
    #[cfg(target_os = "macos")]
    {
        if !WAS_PLAYING.load(Ordering::Acquire) {
            return;
        }
        // Let audio quality settle after recording before resuming playback.
        std::thread::sleep(std::time::Duration::from_millis(500));
        toggle("Music");
        toggle("Spotify");
        WAS_PLAYING.store(false, Ordering::Release);
    }
}
