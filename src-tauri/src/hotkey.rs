use crate::audio::{DiscardReason, StopOutcome};
use crate::recorder::Recorder;
use crate::tray::{self, TrayState};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField,
};
use std::sync::Arc;
use tauri::{tray::TrayIcon, AppHandle, Emitter};

fn key_for_name(name: &str) -> (i64, CGEventFlags) {
    match name {
        "right_control" => (0x3E, CGEventFlags::CGEventFlagControl),
        "right_command" => (0x36, CGEventFlags::CGEventFlagCommand),
        "right_shift"   => (0x3C, CGEventFlags::CGEventFlagShift),
        _               => (0x3D, CGEventFlags::CGEventFlagAlternate), // right_option (default)
    }
}

/// Emit a UI-critical event. Logs at warn-level if the emit fails (e.g. no
/// frontend listener is registered yet). Non-critical signals like
/// `audio-level` can keep using fire-and-forget `let _ = app.emit(...)`.
fn emit_critical<P: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: P) {
    if let Err(e) = app.emit(event, payload) {
        tracing::warn!("[hotkey] failed to emit {}: {:?}", event, e);
    }
}

// All work that touches the audio pipeline must run off the CGEventTap thread.
// macOS disables event taps whose callback exceeds the per-event timeout, and
// `recorder.start()` (cpal stream open) plus `recorder.stop()` (downmix +
// resample + Silero VAD inference + WAV write) can take hundreds of ms to
// several seconds — well over the timeout. Synchronous calls in the tap
// callback led to the tap being disabled after 1-2 recordings, after which
// no key event reaches our code at all (silent dead hotkey).
//
// Both ptt_down and ptt_up therefore spawn a worker thread and return
// immediately. The tap callback finishes in microseconds.

fn ptt_down(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
    let rec  = recorder.clone();
    let tray = tray_icon.clone();
    let app  = app.clone();
    std::thread::spawn(move || {
        // One-in-flight policy: only `Ready` is allowed to start a new job.
        // If the recorder is busy (anything from FinalizingAudio through
        // Pasting still running from a prior press), report it as
        // `dictation-busy` so the UI/user can observe the dropped press
        // without us silently swallowing it.
        let snapshot = rec.state();
        if snapshot.is_busy() {
            tracing::warn!("[hotkey] start ignored — recorder busy in {}", snapshot);
            emit_critical(&app, "dictation-busy", snapshot.to_string());
            return;
        }
        if let Err(e) = rec.start() {
            // Race: state moved out of Ready between our snapshot and the
            // start() call (e.g. another press won the lock first), or audio
            // backend failed. Do NOT emit ptt-down, do NOT change tray icon.
            tracing::warn!("[hotkey] start ignored: {}", e);
            emit_critical(&app, "dictation-busy", rec.state().to_string());
            return;
        }
        let _ = tray.set_icon(Some(tray::make_icon(TrayState::Recording)));
        emit_critical(&app, "ptt-down", ());
    });
}

fn ptt_up(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
    let rec  = recorder.clone();
    let tray = tray_icon.clone();
    let app  = app.clone();
    std::thread::spawn(move || {
        // Tray-state policy: Recording icon only during literal capture; the
        // moment we enter FinalizingAudio (inside `rec.stop()`) the tray flips
        // to Transcribing and stays that way through Cleaning + Pasting.
        // Idle is restored exactly once at the end of the lifecycle.
        match rec.stop() {
            Ok(StopOutcome::Wav { path }) => {
                // We are now in `FinalizingAudio` per recorder contract.
                let _ = tray.set_icon(Some(tray::make_icon(TrayState::Transcribing)));
                emit_critical(&app, "ptt-up", ());

                // `path` is a tempfile::TempPath — its Drop deletes the WAV
                // automatically whether we exit via the success arm, the
                // error arm, or a panic. No explicit cleanup needed.

                // FinalizingAudio → Transcribing
                if let Err(e) = rec.begin_transcribing() {
                    tracing::error!("[hotkey] begin_transcribing failed: {}", e);
                    rec.finish();
                    let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                    return;
                }

                let transcribe_result = crate::transcribe::run(&path);

                // NOTE: TASK-15 will split the cleanup pass out of
                // `transcribe::run` so the Cleaning state covers the cleanup
                // call only. Today cleanup runs inside `transcribe::run`, so
                // we walk Transcribing → Cleaning at the same instant —
                // structure for the future, accuracy will follow.
                if rec.begin_cleaning().is_err() {
                    // Should be unreachable given begin_transcribing succeeded,
                    // but if it does happen the recorder has been forced out
                    // from under us (e.g. cancel). Bail cleanly.
                    rec.finish();
                    let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                    return;
                }

                match transcribe_result {
                    Ok(text) => {
                        tracing::info!("[transcribe] {:?}", text);
                        emit_critical(&app, "transcript", text.clone());

                        // Cleaning → Pasting
                        if rec.begin_pasting().is_err() {
                            rec.finish();
                            let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                            return;
                        }

                        if let Err(e) = crate::paste::paste(&text) {
                            tracing::error!("[paste] {:?}", e);
                            // Surface to UI so the user knows the transcript
                            // was processed but never reached the focused app.
                            let msg = "Couldn't paste — check Accessibility permission".to_string();
                            emit_critical(&app, "paste-error", msg);
                        }
                    }
                    Err(e) => {
                        tracing::error!("[transcribe] {:?}", e);
                        let msg = format!("{}", e);
                        emit_critical(&app, "transcript-error", msg);
                    }
                }

                // End of lifecycle — back to Ready regardless of which arm we
                // took (success, transcribe error, or paste error).
                rec.finish();
                let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                // `path` drops here → WAV file deleted from /tmp.
            }
            Ok(StopOutcome::Discard(reason)) => {
                // `rec.stop()` already returned us to Ready on the discard arm.
                // Tell the frontend to clear its overlay — otherwise it stays
                // stuck on "Transcribing…". `recording-discarded` is the
                // catch-all the overlay listens to; `recording-too-short` is
                // the more specific subtype the main window uses to show a
                // duration-aware toast.
                let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                if let DiscardReason::TooShort { duration_ms } = reason {
                    emit_critical(&app, "recording-too-short", duration_ms);
                }
                emit_critical(&app, "recording-discarded", ());
            }
            Err(e) => {
                // Illegal transition (e.g. stop while not Recording) or audio
                // pipeline error. The recorder has already returned itself to
                // Ready on the error arm. Do NOT emit ptt-up. Restore the tray
                // defensively in case this was an audio failure mid-job.
                tracing::warn!("[hotkey] stop ignored: {}", e);
                let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
            }
        }
    });
}

pub fn spawn(
    recorder: Arc<Recorder>,
    tray_icon: TrayIcon,
    app: AppHandle,
    hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
) {
    std::thread::spawn(move || {
        let tap = match CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::FlagsChanged],
            move |_proxy, _etype, event| {
                let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                let flags   = event.get_flags();

                // Read current config (RwLock read — nanoseconds, uncontended)
                let (target_keycode, target_flag, toggle_mode) = {
                    let hk = hotkey_state.read();
                    let (kc, f) = key_for_name(&hk.key);
                    (kc, f, hk.mode == "toggle")
                };

                if keycode == target_keycode {
                    let is_key_down = flags.contains(target_flag);
                    if toggle_mode {
                        if is_key_down {
                            if recorder.is_recording() {
                                ptt_up(&recorder, &tray_icon, &app);
                            } else {
                                ptt_down(&recorder, &tray_icon, &app);
                            }
                        }
                    } else if is_key_down {
                        ptt_down(&recorder, &tray_icon, &app);
                    } else {
                        ptt_up(&recorder, &tray_icon, &app);
                    }
                }
                None
            },
        ) {
            Ok(t) => t,
            Err(()) => {
                tracing::error!(
                    "[hotkey] CGEventTap failed — grant Accessibility permission in \
                     System Settings → Privacy & Security → Accessibility, then restart"
                );
                return;
            }
        };

        let source = tap
            .mach_port
            .create_runloop_source(0)
            .expect("[hotkey] create_runloop_source failed");
        // SAFETY: kCFRunLoopCommonModes is a static CFStringRef constant exported by
        // core-foundation. Reading it requires unsafe because the binding is a static
        // extern, but the value is immutable and thread-safe to read.
        CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        CFRunLoop::run_current();
    });
}
