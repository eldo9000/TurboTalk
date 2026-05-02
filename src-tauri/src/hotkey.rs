use crate::audio::{DiscardReason, StopOutcome};
use crate::recorder::Recorder;
use crate::tray::{self, TrayState};
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{tray::TrayIcon, AppHandle, Emitter};

/// Monotonically-increasing identifier attached to every accepted dictation
/// job. Incremented exactly once per successful `recorder.start()` so that
/// backend logs and frontend `dictation-stage` events for a single press all
/// carry the same `job_id`. Wraps after ~1.8e19 jobs — i.e. never.
static JOB_ID: AtomicU64 = AtomicU64::new(0);

/// Allocate the next `job_id`. First call returns 1.
fn next_job_id() -> u64 {
    JOB_ID.fetch_add(1, Ordering::Relaxed) + 1
}

/// Payload for the additive `dictation-stage` event introduced in TASK-15.
/// Frontend listeners may ignore this entirely; existing events
/// (`ptt-down`, `ptt-up`, `transcript`, `recording-discarded`, …) are
/// preserved unchanged for backward compatibility.
#[derive(Clone, serde::Serialize)]
struct DictationStage {
    job_id: u64,
    stage: &'static str,
}

fn emit_stage(app: &AppHandle, job_id: u64, stage: &'static str) {
    tracing::debug!("[dictation job_id={}] stage={}", job_id, stage);
    emit_critical(app, "dictation-stage", DictationStage { job_id, stage });
}

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

/// Cell shared between `ptt_down` and `ptt_up` so the upstroke worker can
/// recover the `job_id` allocated by the downstroke worker. Holds `None`
/// when no recording is in flight. Guarded by `parking_lot::Mutex`; the
/// critical section is a single load/store and never blocks audio work.
static CURRENT_JOB_ID: parking_lot::Mutex<Option<u64>> = parking_lot::Mutex::new(None);

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
        // Recording was accepted — allocate this job's id.
        let job_id = next_job_id();
        *CURRENT_JOB_ID.lock() = Some(job_id);
        tracing::info!("[hotkey job_id={}] recording started", job_id);

        let _ = tray.set_icon(Some(tray::make_icon(TrayState::Recording)));
        emit_critical(&app, "ptt-down", ());
        emit_stage(&app, job_id, "recording");
    });
}

fn ptt_up(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
    let rec  = recorder.clone();
    let tray = tray_icon.clone();
    let app  = app.clone();
    std::thread::spawn(move || {
        // Recover the job id allocated when this recording started. If the
        // upstroke arrives without a matching downstroke (no in-flight job),
        // we still call `rec.stop()` defensively but skip stage emissions.
        let job_id_opt = CURRENT_JOB_ID.lock().take();

        // Tray-state policy: Recording icon only during literal capture; the
        // moment we enter FinalizingAudio (inside `rec.stop()`) the tray flips
        // to Transcribing and stays that way through Cleaning + Pasting.
        // Idle is restored exactly once at the end of the lifecycle.
        match rec.stop() {
            Ok(StopOutcome::Wav { path }) => {
                // We are now in `FinalizingAudio` per recorder contract.
                let _ = tray.set_icon(Some(tray::make_icon(TrayState::Transcribing)));
                emit_critical(&app, "ptt-up", ());
                if let Some(job_id) = job_id_opt {
                    emit_stage(&app, job_id, "finalizing_audio");
                }

                // `path` is a tempfile::TempPath — its Drop deletes the WAV
                // automatically whether we exit via the success arm, the
                // error arm, or a panic. No explicit cleanup needed.

                // FinalizingAudio → Transcribing
                if let Err(e) = rec.begin_transcribing() {
                    tracing::error!(
                        "[hotkey job_id={:?}] begin_transcribing failed: {}",
                        job_id_opt, e
                    );
                    rec.finish();
                    let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                    if let Some(job_id) = job_id_opt {
                        emit_stage(&app, job_id, "ready");
                    }
                    return;
                }
                if let Some(job_id) = job_id_opt {
                    emit_stage(&app, job_id, "transcribing");
                }

                // Stage 1: raw whisper transcription (no cleanup).
                let transcribe_result = crate::transcribe::run_raw(&path);

                // Transcribing → Cleaning (always — even on whisper error,
                // so the lifecycle reaches `finish` through legal transitions).
                if rec.begin_cleaning().is_err() {
                    // Should be unreachable given begin_transcribing succeeded,
                    // but if it does happen the recorder has been forced out
                    // from under us (e.g. cancel). Bail cleanly.
                    rec.finish();
                    let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                    if let Some(job_id) = job_id_opt {
                        emit_stage(&app, job_id, "ready");
                    }
                    return;
                }
                if let Some(job_id) = job_id_opt {
                    emit_stage(&app, job_id, "cleaning");
                }

                match transcribe_result {
                    Ok(raw_text) => {
                        tracing::info!(
                            "[transcribe job_id={:?}] raw {:?}",
                            job_id_opt, raw_text
                        );

                        // Stage 2: cleanup as its own explicit call site.
                        let final_text = crate::cleanup::process(&raw_text);
                        tracing::info!(
                            "[cleanup   job_id={:?}] final {:?}",
                            job_id_opt, final_text
                        );
                        emit_critical(&app, "transcript", final_text.clone());

                        // Cleaning → Pasting
                        if rec.begin_pasting().is_err() {
                            rec.finish();
                            let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                            if let Some(job_id) = job_id_opt {
                                emit_stage(&app, job_id, "ready");
                            }
                            return;
                        }
                        if let Some(job_id) = job_id_opt {
                            emit_stage(&app, job_id, "pasting");
                        }

                        // Stage 3: paste into the focused app.
                        if let Err(e) = crate::paste::paste(&final_text) {
                            tracing::error!("[paste job_id={:?}] {:?}", job_id_opt, e);
                            // Surface to UI so the user knows the transcript
                            // was processed but never reached the focused app.
                            let msg = "Couldn't paste — check Accessibility permission".to_string();
                            emit_critical(&app, "paste-error", msg);
                        }
                    }
                    Err(e) => {
                        tracing::error!("[transcribe job_id={:?}] {:?}", job_id_opt, e);
                        let msg = format!("{}", e);
                        emit_critical(&app, "transcript-error", msg);
                    }
                }

                // End of lifecycle — back to Ready regardless of which arm we
                // took (success, transcribe error, or paste error).
                rec.finish();
                let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                if let Some(job_id) = job_id_opt {
                    emit_stage(&app, job_id, "ready");
                }
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
                if let Some(job_id) = job_id_opt {
                    emit_stage(&app, job_id, "ready");
                }
            }
            Err(e) => {
                // Illegal transition (e.g. stop while not Recording) or audio
                // pipeline error. The recorder has already returned itself to
                // Ready on the error arm. Do NOT emit ptt-up. Restore the tray
                // defensively in case this was an audio failure mid-job.
                tracing::warn!("[hotkey job_id={:?}] stop ignored: {}", job_id_opt, e);
                let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                if let Some(job_id) = job_id_opt {
                    emit_stage(&app, job_id, "ready");
                }
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
