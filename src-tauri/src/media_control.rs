// Pause/resume media playback when dictation starts/stops.
// Posts NXSYSDEFINED media key event from inline C (same process,
// same Accessibility permissions as main app).
// Detects real playback via a CoreAudio process tap and sample-energy
// threshold. State APIs like MediaRemote/Now Playing and CoreAudio
// "is running" flags are too narrow or too stale for Chrome/YouTube.

use std::sync::atomic::{AtomicBool, Ordering};
use std::{ffi::CStr, os::raw::c_char};

static DID_PAUSE: AtomicBool = AtomicBool::new(false);

const POST_PAUSE_SETTLE_MS: u64 = 250;
const ROUTE_RESTORE_TIMEOUT_MS: i32 = 2_500;

#[cfg(target_os = "macos")]
extern "C" {
    fn media_toggle_play_pause();
    fn audio_is_playing() -> i32;
    fn audio_probe_last_samples() -> u64;
    fn audio_probe_last_rms() -> f64;
    fn audio_probe_last_peak() -> f64;
    fn audio_probe_last_status() -> i32;
    fn audio_probe_last_diag() -> *const c_char;
    fn audio_route_capture_output_baseline() -> i32;
    fn audio_route_wait_for_output_baseline(max_wait_ms: i32) -> i32;
    fn audio_route_last_diag() -> *const c_char;
    fn audio_probe_process_tap_available() -> i32;
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaybackProbe {
    Playing,
    Silent,
    Unavailable,
}

#[cfg(target_os = "macos")]
fn route_diag() -> String {
    unsafe {
        let ptr = audio_route_last_diag();
        if ptr.is_null() {
            "(null)".to_string()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
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
                std::thread::sleep(std::time::Duration::from_millis(POST_PAUSE_SETTLE_MS));
                let baseline_ok = unsafe { audio_route_capture_output_baseline() };
                tracing::info!(
                    "[media_control] route baseline captured={} diag={}",
                    baseline_ok,
                    route_diag()
                );
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

/// Minimal probe: tries to create a process tap to check whether system-audio-
/// capture permission is available. Returns 1 = granted, 0 = denied/unavailable,
/// -1 = unsupported (macOS < 14.2). On first call triggers the macOS TCC dialog
/// if permission has not yet been determined.
#[cfg(target_os = "macos")]
pub fn probe_system_audio_permission() -> i32 {
    unsafe { audio_probe_process_tap_available() }
}

/// Resume media playback. Call after dictation finishes.
pub fn resume() {
    #[cfg(target_os = "macos")]
    {
        if !DID_PAUSE.load(Ordering::Acquire) {
            tracing::debug!("[media_control] resume — skipped (nothing was paused)");
            return;
        }
        let route_ready = unsafe { audio_route_wait_for_output_baseline(ROUTE_RESTORE_TIMEOUT_MS) };
        tracing::info!(
            "[media_control] resume — route_ready={} diag={}",
            route_ready,
            route_diag()
        );
        unsafe { media_toggle_play_pause() }
        DID_PAUSE.store(false, Ordering::Release);
    }
}
