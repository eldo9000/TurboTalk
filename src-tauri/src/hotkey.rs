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
use std::time::Instant;
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

/// Payload for the additive `focus-changed-before-paste` event introduced
/// in TASK-16. Emitted only when the frontmost-app identifier captured at
/// recording start differs from the one captured immediately before paste.
/// Both fields may be `None` if the macOS query failed at either capture
/// site — see `paste::frontmost_app`. Default policy is "paste anyway,
/// observe the change"; the frontend uses this to surface a gentle banner.
#[derive(Clone, serde::Serialize)]
struct FocusChangedBeforePaste {
    job_id: u64,
    focus_at_start: Option<String>,
    focus_at_paste: Option<String>,
}

/// Cell shared between `ptt_down` and `ptt_up` so the upstroke worker can
/// recover the frontmost-app identifier captured when this recording
/// started. Holds `None` when no recording is in flight; the inner
/// `Option<String>` may itself be `None` if the macOS query failed.
/// Guarded by `parking_lot::Mutex`; the critical section is one load/store.
static FOCUS_AT_START: parking_lot::Mutex<Option<Option<String>>> =
    parking_lot::Mutex::new(None);

fn key_for_name(name: &str) -> (i64, CGEventFlags) {
    match name {
        "right_control" => (0x3E, CGEventFlags::CGEventFlagControl),
        "right_command" => (0x36, CGEventFlags::CGEventFlagCommand),
        "right_shift"   => (0x3C, CGEventFlags::CGEventFlagShift),
        _               => (0x3D, CGEventFlags::CGEventFlagAlternate), // right_option (default)
    }
}

// ── TASK-23: cancel-chord helpers ────────────────────────────────────────────

/// keycode for the Esc key on macOS (matches Carbon kVK_Escape = 0x35).
const ESC_KEYCODE: i64 = 0x35;

/// How long Ctrl+Alt must be held alone before the cancel fires (TASK-23).
const CANCEL_CHORD_HOLD_MS: u128 = 300;

/// The exact modifier combination that activates the cancel chord:
/// Control + Alternate, with no other modifier bits set.
///
/// We compare against `CANCEL_CHORD_MASK` by masking out the bits we care
/// about and checking they equal the expected combination. This correctly
/// handles "NumLock always set" and similar platform quirks.
const CANCEL_CHORD_MASK: CGEventFlags = CGEventFlags::CGEventFlagControl
    .union(CGEventFlags::CGEventFlagAlternate);

/// Modifier bits that must NOT be set for the chord to be "clean". If any of
/// these are present alongside Ctrl+Alt, we do not start (or continue) the
/// hold timer — the user is probably typing Ctrl+Alt+<letter>.
const EXCLUSIVE_MODIFIER_BITS: CGEventFlags = CGEventFlags::CGEventFlagCommand
    .union(CGEventFlags::CGEventFlagShift);

/// Returns true iff `flags` contains exactly Control+Alternate with no
/// Command or Shift bit set. Used in tests and in the event-tap callback.
pub fn cancel_chord_active(flags: CGEventFlags) -> bool {
    let has_chord = flags.contains(CANCEL_CHORD_MASK);
    let no_extra  = !flags.intersects(EXCLUSIVE_MODIFIER_BITS);
    has_chord && no_extra
}

/// Fire the cancel path: call `recorder.cancel()`, reset tray, emit
/// `recording-cancelled` (no payload). Called from both the chord-debounce
/// polling thread and the Esc-keydown path.
fn do_cancel(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
    // Only cancel if actually in-flight (Recording or Transcribing).
    let state = recorder.state();
    if !matches!(
        state,
        crate::recorder::State::Recording | crate::recorder::State::Transcribing
    ) {
        tracing::debug!(
            "[hotkey] cancel gesture ignored — recorder in {} (not Recording/Transcribing)",
            state
        );
        return;
    }
    recorder.cancel();
    tracing::info!("[hotkey] recording cancelled by user gesture");
    let _ = tray_icon.set_icon(Some(tray::make_icon(TrayState::Idle)));
    emit_critical(app, "recording-cancelled", ());
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

        // Capture the frontmost app at recording start (best-effort; may be
        // `None` if osascript fails). Stored under its own mutex so the
        // upstroke worker can recover it without entangling JOB_ID's lock.
        // Logged with the job_id so a single grep gives the full lifecycle.
        let focus_start = crate::paste::frontmost_app();
        *FOCUS_AT_START.lock() = Some(focus_start.clone());
        tracing::info!(
            "[hotkey job_id={}] recording started focus_at_start={:?}",
            job_id, focus_start
        );

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
        // Recover the focus identity captured at recording start. Outer
        // `Option` = "was a recording in flight"; inner `Option<String>` =
        // "did the macOS query succeed". We only compare-and-emit later if
        // we actually have a job and reach the paste stage.
        let focus_at_start: Option<String> = FOCUS_AT_START.lock().take().flatten();

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
                        //
                        // Focus policy (TASK-16): we paste into whatever app
                        // is frontmost *at this moment*, not at recording
                        // start. Capture the current frontmost app first so
                        // we can log and surface focus changes to the UI
                        // without blocking the paste itself. See
                        // ARCHITECTURE.md → "Paste Target Policy".
                        let focus_at_paste = crate::paste::frontmost_app();
                        tracing::info!(
                            "[paste job_id={:?}] focus_at_start={:?} focus_at_paste={:?}",
                            job_id_opt, focus_at_start, focus_at_paste
                        );
                        if let (Some(job_id), Some(start), Some(now)) =
                            (job_id_opt, focus_at_start.as_ref(), focus_at_paste.as_ref())
                        {
                            if start != now {
                                tracing::warn!(
                                    "[paste job_id={}] focus changed before paste: {:?} → {:?}",
                                    job_id, start, now
                                );
                                emit_critical(
                                    &app,
                                    "focus-changed-before-paste",
                                    FocusChangedBeforePaste {
                                        job_id,
                                        focus_at_start: Some(start.clone()),
                                        focus_at_paste: Some(now.clone()),
                                    },
                                );
                            }
                        }
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
    // TASK-23: shared cell tracking when the Ctrl+Alt chord was entered.
    // `None` = chord not active; `Some(t)` = chord entered at time t.
    // Wrapped in Arc<Mutex> so the polling thread (below) can read it.
    let chord_entered_at: Arc<parking_lot::Mutex<Option<Instant>>> =
        Arc::new(parking_lot::Mutex::new(None));

    // Polling thread: every 50 ms, check whether the Ctrl+Alt chord has been
    // held for >= 300 ms with no intervening key events. If so, fire cancel.
    // This piggybacks on the same 50 ms tick used by the audio-level broadcaster.
    {
        let chord_cell = chord_entered_at.clone();
        let rec        = recorder.clone();
        let tray       = tray_icon.clone();
        let app2       = app.clone();
        let hs         = hotkey_state.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let cancel_via_ctrl_alt = hs.read().cancel_via_ctrl_alt;
            if !cancel_via_ctrl_alt {
                continue;
            }
            let entered_at = *chord_cell.lock();
            if let Some(t) = entered_at {
                if t.elapsed().as_millis() >= CANCEL_CHORD_HOLD_MS {
                    *chord_cell.lock() = None;
                    do_cancel(&rec, &tray, &app2);
                }
            }
        });
    }

    std::thread::spawn(move || {
        let chord_cell_tap = chord_entered_at.clone();

        let tap = match CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
            move |_proxy, etype, event| {
                let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                let flags   = event.get_flags();

                // Read current config (RwLock read — nanoseconds, uncontended)
                let (target_keycode, target_flag, toggle_mode, cancel_via_ctrl_alt, cancel_via_esc) = {
                    let hk = hotkey_state.read();
                    let (kc, f) = key_for_name(&hk.key);
                    (kc, f, hk.mode == "toggle", hk.cancel_via_ctrl_alt, hk.cancel_via_esc)
                };

                match etype {
                    CGEventType::FlagsChanged => {
                        // PTT hotkey detection (existing behavior)
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

                        // TASK-23: Ctrl+Alt chord tracking
                        if cancel_via_ctrl_alt {
                            if cancel_chord_active(flags) {
                                // Chord entered — start timer if not already started
                                let mut cell = chord_cell_tap.lock();
                                if cell.is_none() {
                                    *cell = Some(Instant::now());
                                }
                            } else {
                                // Chord released — clear the timer
                                *chord_cell_tap.lock() = None;
                            }
                        }
                    }
                    CGEventType::KeyDown => {
                        // Any key down clears the chord timer (deliberate gesture
                        // requires Ctrl+Alt held alone — any third key resets it).
                        *chord_cell_tap.lock() = None;

                        // Esc cancel (opt-in via cancel_via_esc setting)
                        if cancel_via_esc && keycode == ESC_KEYCODE {
                            do_cancel(&recorder, &tray_icon, &app);
                        }
                    }
                    _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    // TASK-23: unit tests for cancel_chord_active against canned CGEventFlags.

    #[test]
    fn cancel_chord_active_true_for_ctrl_alt_only() {
        let flags = CGEventFlags::CGEventFlagControl
            | CGEventFlags::CGEventFlagAlternate;
        assert!(cancel_chord_active(flags), "Ctrl+Alt alone must activate chord");
    }

    #[test]
    fn cancel_chord_active_false_when_shift_added() {
        let flags = CGEventFlags::CGEventFlagControl
            | CGEventFlags::CGEventFlagAlternate
            | CGEventFlags::CGEventFlagShift;
        assert!(!cancel_chord_active(flags), "Ctrl+Alt+Shift must NOT activate chord");
    }

    #[test]
    fn cancel_chord_active_false_when_command_added() {
        let flags = CGEventFlags::CGEventFlagControl
            | CGEventFlags::CGEventFlagAlternate
            | CGEventFlags::CGEventFlagCommand;
        assert!(!cancel_chord_active(flags), "Ctrl+Alt+Cmd must NOT activate chord");
    }

    #[test]
    fn cancel_chord_active_false_for_ctrl_alone() {
        let flags = CGEventFlags::CGEventFlagControl;
        assert!(!cancel_chord_active(flags), "Ctrl alone must NOT activate chord");
    }

    #[test]
    fn cancel_chord_active_false_for_alt_alone() {
        let flags = CGEventFlags::CGEventFlagAlternate;
        assert!(!cancel_chord_active(flags), "Alt alone must NOT activate chord");
    }

    #[test]
    fn cancel_chord_active_false_for_empty_flags() {
        assert!(!cancel_chord_active(CGEventFlags::empty()), "empty flags must NOT activate chord");
    }

    // TASK-23: HotkeyConfig defaults.

    #[test]
    fn hotkey_config_default_cancel_ctrl_alt_on_esc_off() {
        let hk = crate::settings::HotkeyConfig::default();
        assert!(hk.cancel_via_ctrl_alt, "cancel_via_ctrl_alt must default to true");
        assert!(!hk.cancel_via_esc,     "cancel_via_esc must default to false");
    }
}
