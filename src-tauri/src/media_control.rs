// Pause/resume media playback when dictation starts/stops.
// Posts NXSYSDEFINED media key event from inline C (same process,
// same Accessibility permissions as main app).

use std::sync::atomic::{AtomicBool, Ordering};

static DID_PAUSE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
#[link(name = "Cocoa", kind = "framework")]
extern "C" {
    fn media_toggle_play_pause();
}

#[cfg(target_os = "macos")]
fn toggle() {
    unsafe { media_toggle_play_pause() }
}

/// Pause media playback. Call before dictation starts.
pub fn pause() {
    #[cfg(target_os = "macos")]
    {
        toggle();
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
        // Let audio quality settle after recording before resuming playback.
        std::thread::sleep(std::time::Duration::from_millis(500));
        toggle();
        DID_PAUSE.store(false, Ordering::Release);
    }
}
