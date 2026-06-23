// Pause/resume media playback when dictation starts/stops.
// Posts NXSYSDEFINED media key event from inline C (same process,
// same Accessibility permissions as main app).
// Detects active audio via CoreAudio device query.

use std::sync::atomic::{AtomicBool, Ordering};

static DID_PAUSE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
extern "C" {
    fn media_toggle_play_pause();
    fn audio_is_playing() -> i32;
}

/// Check if the system's default audio output has active IO.
/// Works with any media app (Chrome, Music, Spotify, etc.).
#[cfg(target_os = "macos")]
fn any_playing() -> bool {
    unsafe { audio_is_playing() != 0 }
}

/// Pause media playback. Call before dictation starts.
pub fn pause() {
    #[cfg(target_os = "macos")]
    {
        let playing = any_playing();
        if !playing {
            DID_PAUSE.store(false, Ordering::Release);
            return;
        }
        unsafe { media_toggle_play_pause() }
        std::thread::sleep(std::time::Duration::from_millis(200));
        DID_PAUSE.store(true, Ordering::Release);
    }
}

/// Resume media playback. Call after dictation finishes.
pub fn resume() {
    #[cfg(target_os = "macos")]
    {
        if !DID_PAUSE.load(Ordering::Acquire) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(800));
        unsafe { media_toggle_play_pause() }
        DID_PAUSE.store(false, Ordering::Release);
    }
}
