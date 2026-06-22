// Pause/resume media playback when dictation starts/stops.
// Uses osascript to control media players directly.
// MediaRemote private framework is unavailable on macOS 26+.

use std::process::Command;

fn osascript_playpause(app: &str) {
    let _ = Command::new("osascript")
        .args(["-e", &format!(r#"tell application "{}" to playpause"#, app)])
        .output();
}

/// Pause media playback. Call before dictation starts.
pub fn pause() {
    #[cfg(target_os = "macos")]
    {
        // Try the most common media players
        osascript_playpause("Music");
        osascript_playpause("Spotify");
    }
}

/// Resume media playback. Call after dictation finishes.
pub fn resume() {
    #[cfg(target_os = "macos")]
    {
        osascript_playpause("Music");
        osascript_playpause("Spotify");
    }
}
