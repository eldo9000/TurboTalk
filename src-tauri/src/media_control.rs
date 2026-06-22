// Pause/resume media playback when dictation starts/stops.
// Runs inline via osascript as a universal no-crash fallback.

fn osascript_toggle() {
    // Use osascript to send the macOS media play/pause key via HID.
    // Key code 100 is the standard play/pause on Apple keyboards.
    let _ = std::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to key code 100"#,
        ])
        .output();
}

/// Pause media playback. Call before dictation starts.
pub fn pause() {
    #[cfg(target_os = "macos")]
    osascript_toggle();
}

/// Resume media playback. Call after dictation finishes.
pub fn resume() {
    #[cfg(target_os = "macos")]
    osascript_toggle();
}
