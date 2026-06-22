// Pause/resume media playback when dictation starts/stops.
// Posts NXSYSDEFINED media key event from a C function compiled into
// the binary (same process = shares Accessibility permissions).

use std::sync::atomic::{AtomicBool, Ordering};

static WAS_PLAYING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
#[link(name = "Cocoa", kind = "framework")]
extern "C" {
    fn media_toggle_play_pause();
}

#[cfg(target_os = "macos")]
fn toggle() {
    unsafe { media_toggle_play_pause() }
}

#[cfg(target_os = "macos")]
fn any_playing() -> bool {
    for app in &["Music", "Spotify"] {
        let running = std::process::Command::new("pgrep")
            .args(["-x", app])
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !running {
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
pub fn pause() {
    #[cfg(target_os = "macos")]
    {
        if !any_playing() {
            WAS_PLAYING.store(false, Ordering::Release);
            return;
        }
        toggle();
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
        std::thread::sleep(std::time::Duration::from_millis(500));
        toggle();
        WAS_PLAYING.store(false, Ordering::Release);
    }
}
