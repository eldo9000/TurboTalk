// Pause/resume media playback when dictation starts/stops.
// Posts NXSYSDEFINED media key event from inline C (same process,
// same Accessibility permissions as main app).
// Detects real playback via a CoreAudio process tap and sample-energy
// threshold. State APIs like MediaRemote/Now Playing and CoreAudio
// "is running" flags are too narrow or too stale for Chrome/YouTube.

use std::sync::atomic::{AtomicBool, Ordering};
use std::{ffi::CStr, os::raw::c_char};

static DID_PAUSE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
extern "C" {
    fn media_toggle_play_pause();
    fn audio_is_playing() -> i32;
    fn audio_probe_last_samples() -> u64;
    fn audio_probe_last_rms() -> f64;
    fn audio_probe_last_peak() -> f64;
    fn audio_probe_last_status() -> i32;
    fn audio_probe_last_diag() -> *const c_char;
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaybackProbe {
    Playing,
    Silent,
    Unavailable,
}

#[cfg(target_os = "macos")]
fn playback_probe() -> PlaybackProbe {
    let result = unsafe { audio_is_playing() };
    let samples = unsafe { audio_probe_last_samples() };
    let rms = unsafe { audio_probe_last_rms() };
    let peak = unsafe { audio_probe_last_peak() };
    let status = unsafe { audio_probe_last_status() };
    let diag = unsafe {
        let ptr = audio_probe_last_diag();
        if ptr.is_null() {
            "(null)".to_string()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    };
    tracing::info!(
        "[media_control] process tap result={} status={} samples={} rms={:.8} peak={:.8} diag={}",
        result,
        status,
        samples,
        rms,
        peak,
        diag
    );

    match result {
        1 => PlaybackProbe::Playing,
        0 => PlaybackProbe::Silent,
        _ => PlaybackProbe::Unavailable,
    }
}

/// Pause media playback. Call before dictation starts.
pub fn pause() {
    #[cfg(target_os = "macos")]
    {
        match playback_probe() {
            PlaybackProbe::Playing => {
                tracing::info!("[media_control] pause — playback detected, toggling");
                unsafe { media_toggle_play_pause() }
                std::thread::sleep(std::time::Duration::from_millis(200));
                DID_PAUSE.store(true, Ordering::Release);
            }
            PlaybackProbe::Silent => {
                tracing::info!("[media_control] pause — no playback detected, leaving media alone");
                DID_PAUSE.store(false, Ordering::Release);
            }
            PlaybackProbe::Unavailable => {
                tracing::warn!(
                    "[media_control] pause — playback detection unavailable, leaving media alone"
                );
                DID_PAUSE.store(false, Ordering::Release);
            }
        }
    }
}

/// Resume media playback. Call after dictation finishes.
pub fn resume() {
    #[cfg(target_os = "macos")]
    {
        if !DID_PAUSE.load(Ordering::Acquire) {
            tracing::debug!("[media_control] resume — skipped (nothing was paused)");
            return;
        }
        tracing::info!("[media_control] resume — waiting 800ms then toggling");
        std::thread::sleep(std::time::Duration::from_millis(800));
        unsafe { media_toggle_play_pause() }
        DID_PAUSE.store(false, Ordering::Release);
    }
}
