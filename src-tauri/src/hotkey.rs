// Push-to-talk hotkey binding.
//
// Platform facade. The lifecycle (recorder state machine, tray transitions,
// `dictation-stage` events, focus tracking, cancel-pending race handling)
// lives in `mod common` and is shared across all platforms. Each per-OS
// `mod imp` only owns the OS-specific key-event source and calls into
// `common::ptt_down(...)` / `common::ptt_up(...)`.
//
// Public surface preserved by every branch:
//   - `pub fn spawn(recorder, tray_icon, app, hotkey_state)` — the only
//     entry point `lib.rs::run` calls into.
//   - `pub fn accessibility_trusted() -> bool` — used by the onboarding
//     readiness gate.

mod common {
    use crate::audio::{DiscardReason, StopOutcome};
    use crate::recorder::{Recorder, RecorderError};
    use crate::tray::{self, TrayState};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::Arc;
    use tauri::{tray::TrayIcon, AppHandle, Emitter};

    /// Set by `ptt_up` when `rec.stop()` fails because the recorder wasn't
    /// Recording yet (key-up thread won the scheduler over key-down thread in
    /// hold mode). `ptt_down` checks this immediately after `rec.start()` and
    /// cancels the recording instead of showing the overlay, preventing the
    /// "quick tap → overlay stuck forever" bug.
    static CANCEL_PENDING: AtomicBool = AtomicBool::new(false);

    /// Number of pending `ptt_up` invocations that should be silently
    /// swallowed. Each suppressed record-key down stroke (cancel path or
    /// "extend cooldown" tap-mash path) increments this; each `ptt_up` worker
    /// checks-and-decrements it before doing any real work. Counter, not
    /// flag, because a 5–6-tap mash burst arms multiple in a row.
    pub(super) static SUPPRESS_PTT_UP_COUNT: AtomicU32 = AtomicU32::new(0);

    /// Add one to `SUPPRESS_PTT_UP_COUNT`. Call from the listener thread
    /// after suppressing a record-key down dispatch so the matching key-up
    /// is no-op'd by `ptt_up` instead of falling into the IllegalTransition
    /// path and arming `CANCEL_PENDING` for the next press.
    pub(super) fn arm_ptt_up_suppression() {
        SUPPRESS_PTT_UP_COUNT.fetch_add(1, Ordering::Release);
    }

    /// Atomically decrement-if-positive. Returns true if a suppression slot
    /// was consumed (caller should treat the current `ptt_up` as a no-op).
    fn try_consume_ptt_up_suppression() -> bool {
        SUPPRESS_PTT_UP_COUNT
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                if v > 0 {
                    Some(v - 1)
                } else {
                    None
                }
            })
            .is_ok()
    }

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

    #[derive(Clone, serde::Serialize)]
    pub(super) struct UiError {
        pub kind: &'static str,
        pub message: String,
        pub recoverable: bool,
    }

    /// Cell shared between `ptt_down` and `ptt_up` so the upstroke worker can
    /// recover the frontmost-app identifier captured when this recording
    /// started. Holds `None` when no recording is in flight; the inner
    /// `Option<String>` may itself be `None` if the macOS query failed.
    /// Guarded by `parking_lot::Mutex`; the critical section is one load/store.
    static FOCUS_AT_START: parking_lot::Mutex<Option<Option<String>>> =
        parking_lot::Mutex::new(None);

    /// Emit a UI-critical event. Logs at warn-level if the emit fails (e.g. no
    /// frontend listener is registered yet). Non-critical signals like
    /// `audio-level` can keep using fire-and-forget `let _ = app.emit(...)`.
    pub(super) fn emit_critical<P: serde::Serialize + Clone>(
        app: &AppHandle,
        event: &str,
        payload: P,
    ) {
        if let Err(e) = app.emit(event, payload) {
            tracing::warn!("[hotkey] failed to emit {}: {:?}", event, e);
        }
    }

    /// Cell shared between `ptt_down` and `ptt_up` so the upstroke worker can
    /// recover the `job_id` allocated by the downstroke worker. Holds `None`
    /// when no recording is in flight. Guarded by `parking_lot::Mutex`; the
    /// critical section is a single load/store and never blocks audio work.
    static CURRENT_JOB_ID: parking_lot::Mutex<Option<u64>> = parking_lot::Mutex::new(None);

    /// Last observed key-down for the configured PTT key. Read+written from
    /// the OS keyboard listener thread on every record-key press; the lock is
    /// uncontended in practice (one writer, no concurrent readers).
    pub(super) static LAST_RECORD_KEY_DOWN: parking_lot::Mutex<Option<std::time::Instant>> =
        parking_lot::Mutex::new(None);

    /// Window inside which two consecutive record-key down strokes are treated
    /// as a "tap to cancel" gesture rather than two independent presses.
    pub(super) const TAP_CANCEL_WINDOW: std::time::Duration =
        std::time::Duration::from_millis(500);

    /// After a tap-cancel fires, lock out new recordings for this long. Stops
    /// the third / fourth tap in a "mash to cancel" burst from immediately
    /// kicking off a new recording the moment the recorder returns to Ready.
    pub(super) const TAP_CANCEL_COOLDOWN: std::time::Duration =
        std::time::Duration::from_millis(1000);

    /// Timestamp of the last tap-cancel. Compared against `TAP_CANCEL_COOLDOWN`
    /// at the top of `ptt_down` to suppress accidental immediate re-records.
    static LAST_TAP_CANCEL_AT: parking_lot::Mutex<Option<std::time::Instant>> =
        parking_lot::Mutex::new(None);

    /// Mark "tap-cancel just happened" so subsequent record-key presses are
    /// rejected for the cooldown window. Called from the OS listener thread
    /// at the same moment `trigger_cancel` is invoked via the tap path.
    pub(super) fn mark_tap_cancel() {
        *LAST_TAP_CANCEL_AT.lock() = Some(std::time::Instant::now());
    }

    /// Cancel via the rapid-tap gesture. Same teardown as `trigger_cancel`,
    /// plus (a) records the cancel timestamp so `ptt_down` can suppress the
    /// trailing taps in a 2–3-tap burst, and (b) emits a distinct
    /// `recording-cancelled-tap` event so the frontend can tell the gesture
    /// apart from Escape / tray / IPC cancels.
    pub(super) fn trigger_tap_cancel(
        recorder: &Arc<Recorder>,
        tray_icon: &TrayIcon,
        app: &AppHandle,
    ) {
        mark_tap_cancel();
        emit_critical(app, "recording-cancelled-tap", ());
        trigger_cancel(recorder, tray_icon, app);
    }

    /// Cancel any in-flight recording from anywhere outside the normal PTT
    /// flow (Escape key, rapid record-key tap, tray click, IPC command). Does
    /// nothing if the recorder is already `Ready`.
    ///
    /// **Pure cancel** — does NOT arm `SUPPRESS_PTT_UP_COUNT`. Whether to arm
    /// suppression for the matching key-up depends on the hotkey mode (hold
    /// produces a key-up, toggle does not), which only the listener / caller
    /// knows. Callers must call `arm_ptt_up_suppression()` themselves before
    /// calling this when a `ptt_up` should be swallowed.
    pub(super) fn trigger_cancel(
        recorder: &Arc<Recorder>,
        tray_icon: &TrayIcon,
        app: &AppHandle,
    ) {
        if matches!(recorder.state(), crate::recorder::State::Ready) {
            return;
        }
        play_chime(ChimeEvent::Cancel);
        let rec = recorder.clone();
        let tray = tray_icon.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            rec.cancel();
            let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
            emit_critical(&app, "recording-cancelled", ());
        });
    }

    /// Audio cue events. Each maps to one of the four `sound_on_*` config
    /// toggles and a distinct macOS system sound, so the user can keep them
    /// individually on/off and tell them apart by ear.
    #[derive(Clone, Copy)]
    pub(super) enum ChimeEvent {
        Start,
        Transcribe,
        Finish,
        Cancel,
    }

    /// Soft event chime. Played from the backend rather than the frontend
    /// because the overlay window has `focus: false` and ignores cursor
    /// events — WKWebView blocks `AudioContext` audio without a user
    /// gesture, so a frontend Web Audio chime would be silently dropped.
    /// Fires `afplay` on macOS with a built-in system sound; other platforms
    /// silently no-op (good enough for the Tier-1 personal-use scope).
    pub(super) fn play_chime(event: ChimeEvent) {
        let cfg = crate::settings::load();
        let (enabled, sound) = match event {
            // Sound choices are deliberate: short and "soft" by design, and
            // distinct enough that the four events are recognizable by ear.
            //   Pop    — quick percussive "go" for recording start
            //   Morse  — two-blip "thinking" pattern while transcribing
            //   Bottle — gentle glass clink at paste time
            //   Tink   — softest of the system sounds, paired with cancel
            ChimeEvent::Start      => (cfg.sound_on_start,      "/System/Library/Sounds/Pop.aiff"),
            ChimeEvent::Transcribe => (cfg.sound_on_transcribe, "/System/Library/Sounds/Morse.aiff"),
            ChimeEvent::Finish     => (cfg.sound_on_finish,     "/System/Library/Sounds/Bottle.aiff"),
            ChimeEvent::Cancel     => (cfg.sound_on_cancel,     "/System/Library/Sounds/Tink.aiff"),
        };
        if !enabled {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            let vol = cfg.sound_volume.clamp(0.0, 1.0);
            match std::process::Command::new("afplay")
                .arg("-v")
                .arg(format!("{:.3}", vol))
                .arg(sound)
                .spawn()
            {
                Ok(_) => tracing::info!(
                    "[chime] afplay {} (vol={:.2})",
                    sound.rsplit('/').next().unwrap_or(sound),
                    vol
                ),
                Err(e) => tracing::warn!("[chime] afplay failed for {}: {}", sound, e),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (cfg, sound);
        }
    }

    // All work that touches the audio pipeline must run off the listener thread.
    // On macOS the CGEventTap callback is timeout-bounded; on Windows/Linux the
    // `rdev::listen` callback runs on the listener thread and any blocking work
    // there would stall the global keyboard hook. Both ptt_down and ptt_up
    // therefore spawn a worker thread and return immediately.

    pub(super) fn ptt_down(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
        let rec = recorder.clone();
        let tray = tray_icon.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            // Tap-cancel cooldown: a "mash the record key to cancel" gesture
            // ends with the user having tapped one or two times PAST the cancel
            // — we don't want those trailing taps to immediately kick off a
            // fresh recording. Skip the press silently if we're inside the
            // cooldown window. Logged so it's visible in the trace, not
            // emitted as `dictation-busy` (the press wasn't a real start
            // attempt — it was the tail of a cancel burst).
            if let Some(t) = *LAST_TAP_CANCEL_AT.lock() {
                if t.elapsed() < TAP_CANCEL_COOLDOWN {
                    tracing::info!(
                        "[hotkey] start ignored — within tap-cancel cooldown ({} ms left)",
                        (TAP_CANCEL_COOLDOWN - t.elapsed()).as_millis()
                    );
                    return;
                }
            }
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
            // Recording was accepted. Check if key-up already arrived while
            // this thread was waiting to be scheduled (quick-tap race in hold
            // mode). If so, cancel immediately — don't show the overlay.
            if CANCEL_PENDING.swap(false, Ordering::AcqRel) {
                rec.cancel();
                let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                return;
            }

            // Allocate this job's id.
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
                job_id,
                focus_start
            );

            let _ = tray.set_icon(Some(tray::make_icon(TrayState::Recording)));
            emit_critical(&app, "ptt-down", ());
            emit_stage(&app, job_id, "recording");
            play_chime(ChimeEvent::Start);
        });
    }

    pub(super) fn ptt_up(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
        let rec = recorder.clone();
        let tray = tray_icon.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            // Cancel-cascade suppression: any cancel path or tap-mash listener
            // that suppressed a record-key down dispatch armed one slot in
            // `SUPPRESS_PTT_UP_COUNT`. Each such pairing's key-up arrives here
            // with the recorder already returned to Ready (or never started).
            // Drop on the floor so we don't fall into IllegalTransition and
            // arm CANCEL_PENDING for the next press.
            if try_consume_ptt_up_suppression() {
                let _ = CURRENT_JOB_ID.lock().take();
                let _ = FOCUS_AT_START.lock().take();
                return;
            }
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
                            job_id_opt,
                            e
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
                    play_chime(ChimeEvent::Transcribe);

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
                                "[transcribe job_id={:?}] raw transcript received ({} chars)",
                                job_id_opt,
                                raw_text.chars().count()
                            );

                            // Stage 2: cleanup as its own explicit call site.
                            let final_text = crate::cleanup::process(&raw_text, &app);
                            tracing::info!(
                                "[cleanup   job_id={:?}] final transcript ready ({} chars)",
                                job_id_opt,
                                final_text.chars().count()
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
                                job_id_opt,
                                focus_at_start,
                                focus_at_paste
                            );
                            if let (Some(job_id), Some(start), Some(now)) =
                                (job_id_opt, focus_at_start.as_ref(), focus_at_paste.as_ref())
                            {
                                if start != now {
                                    tracing::warn!(
                                        "[paste job_id={}] focus changed before paste: {:?} → {:?}",
                                        job_id,
                                        start,
                                        now
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
                                let msg =
                                    "Couldn't paste — check Accessibility permission".to_string();
                                emit_critical(&app, "paste-error", msg);
                            } else {
                                play_chime(ChimeEvent::Finish);
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
                    // If stop() failed because the recorder wasn't in Recording
                    // state yet, this is the quick-tap race: our thread ran before
                    // ptt_down's thread called start(). Set CANCEL_PENDING so that
                    // ptt_down cancels as soon as it starts rather than leaving the
                    // overlay stuck with no future ptt_up to clean it up.
                    if matches!(
                        e,
                        RecorderError::IllegalTransition {
                            from: crate::recorder::State::Ready,
                            ..
                        }
                    ) {
                        CANCEL_PENDING.store(true, Ordering::Release);
                    }
                    tracing::warn!("[hotkey job_id={:?}] stop ignored: {}", job_id_opt, e);
                    let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                    if let Some(job_id) = job_id_opt {
                        emit_stage(&app, job_id, "ready");
                    }
                }
            }
        });
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::common;
    use crate::recorder::Recorder;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType, EventField,
    };
    use std::sync::Arc;

    use tauri::{tray::TrayIcon, AppHandle};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    pub fn accessibility_trusted() -> bool {
        // SAFETY: AXIsProcessTrusted takes no pointers and only reports the
        // current process' macOS Accessibility trust state.
        unsafe { AXIsProcessTrusted() }
    }

    fn key_for_name(name: &str) -> (i64, CGEventFlags) {
        match name {
            "right_control" => (0x3E, CGEventFlags::CGEventFlagControl),
            "right_command" => (0x36, CGEventFlags::CGEventFlagCommand),
            "right_shift" => (0x3C, CGEventFlags::CGEventFlagShift),
            _ => (0x3D, CGEventFlags::CGEventFlagAlternate), // right_option (default)
        }
    }

    /// kVK_Escape from `Carbon/HIToolbox/Events.h`.
    const ESCAPE_KEYCODE: i64 = 0x35;

    pub fn spawn(
        recorder: Arc<Recorder>,
        tray_icon: TrayIcon,
        app: AppHandle,
        hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
    ) {
        std::thread::spawn(move || {
            let app_for_callback = app.clone();
            let app_for_error = app.clone();
            let tap = match CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
                move |_proxy, etype, event| {
                    let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                    let flags = event.get_flags();

                    // Read current config (RwLock read — nanoseconds, uncontended)
                    let (target_keycode, target_flag, toggle_mode, cancel_on_esc) = {
                        let hk = hotkey_state.read();
                        let (kc, f) = key_for_name(&hk.key);
                        (kc, f, hk.mode == "toggle", hk.cancel_on_esc)
                    };

                    // Escape → cancel any in-flight recording. Read-only on
                    // events outside Recording/Transcribing so it never
                    // swallows Escape from the focused app while idle.
                    if let CGEventType::KeyDown = etype {
                        if cancel_on_esc && keycode == ESCAPE_KEYCODE {
                            let s = recorder.state();
                            if matches!(
                                s,
                                crate::recorder::State::Recording
                                    | crate::recorder::State::Transcribing
                            ) {
                                // If the user is still holding the record key
                                // (hold mode + Recording), the matching key-up
                                // will fire ptt_up; arm one slot so it no-ops
                                // instead of cascading into CANCEL_PENDING.
                                if !toggle_mode
                                    && matches!(s, crate::recorder::State::Recording)
                                {
                                    common::arm_ptt_up_suppression();
                                }
                                common::trigger_cancel(
                                    &recorder, &tray_icon, &app_for_callback,
                                );
                            }
                        }
                    }

                    if let CGEventType::FlagsChanged = etype {
                        if keycode == target_keycode {
                            let is_key_down = flags.contains(target_flag);
                            // Tap-cancel: in toggle mode only, two record-key
                            // down strokes within `TAP_CANCEL_WINDOW` of each
                            // other are read as a "mash to cancel" safety
                            // gesture. Hold mode doesn't need this — release
                            // already stops the recording cleanly. Always on
                            // in toggle mode (no opt-out): it's a safety net
                            // for accidental double-toggles.
                            if is_key_down && toggle_mode {
                                let now = std::time::Instant::now();
                                let mut last = common::LAST_RECORD_KEY_DOWN.lock();
                                let recent = last
                                    .map(|t| now.duration_since(t) < common::TAP_CANCEL_WINDOW)
                                    .unwrap_or(false);
                                *last = Some(now);
                                drop(last);
                                if recent {
                                    // First "recent" tap during a busy
                                    // recorder cancels; subsequent taps in
                                    // the burst extend the cooldown so a
                                    // 5- or 6-tap anxious mash can't slip
                                    // past into a fresh recording. Either
                                    // branch suppresses normal PTT dispatch
                                    // for this stroke; toggle-mode releases
                                    // are already no-ops, so no ptt_up
                                    // suppression is needed.
                                    let s = recorder.state();
                                    let do_cancel = matches!(
                                        s,
                                        crate::recorder::State::Recording
                                            | crate::recorder::State::Transcribing
                                    );
                                    if do_cancel {
                                        common::trigger_tap_cancel(
                                            &recorder, &tray_icon, &app_for_callback,
                                        );
                                    } else {
                                        common::mark_tap_cancel();
                                    }
                                    return None;
                                }
                            }
                            if toggle_mode {
                                if is_key_down {
                                    if recorder.is_recording() {
                                        common::ptt_up(&recorder, &tray_icon, &app_for_callback);
                                    } else {
                                        common::ptt_down(&recorder, &tray_icon, &app_for_callback);
                                    }
                                }
                            } else if is_key_down {
                                common::ptt_down(&recorder, &tray_icon, &app_for_callback);
                            } else {
                                common::ptt_up(&recorder, &tray_icon, &app_for_callback);
                            }
                        }
                    }

                    None
                },
            ) {
                Ok(t) => t,
                Err(()) => {
                    let trusted = accessibility_trusted();
                    let message = if trusted {
                        "Record trigger failed to start. Restart Turbo Talk and try again."
                    } else {
                        "Record trigger needs Accessibility permission. Add Turbo Talk in System Settings -> Privacy & Security -> Accessibility, then quit and reopen the app."
                    };
                    tracing::error!("[hotkey] CGEventTap failed (accessibility_trusted={trusted})");
                    let app_for_emit = app_for_error.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(1200));
                        common::emit_critical(
                            &app_for_emit,
                            "ui-error",
                            common::UiError {
                                kind: "hotkey-permission",
                                message: message.to_string(),
                                recoverable: true,
                            },
                        );
                    });
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
}

#[cfg(not(target_os = "macos"))]
mod imp {
    //! Windows + Linux/X11 push-to-talk via `rdev`.
    //!
    //! `rdev::listen` installs a global keyboard hook (Win32 `SetWindowsHookEx`
    //! on Windows, X11 `XRecord` on Linux) and invokes the supplied callback
    //! for every key event system-wide. We translate `KeyPress` / `KeyRelease`
    //! against the configured hotkey into `common::ptt_down` / `common::ptt_up`.
    //!
    //! Wayland: `rdev` only supports X11. If `XDG_SESSION_TYPE=wayland` is set
    //! we do NOT call `rdev::listen` (it would either crash or silently fail
    //! to receive events). Instead we emit a `ui-error` so the frontend can
    //! surface a clear "Wayland not supported" message.

    use super::common;
    use crate::recorder::Recorder;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tauri::{tray::TrayIcon, AppHandle};

    pub fn accessibility_trusted() -> bool {
        // Windows: no equivalent permission gate — global hooks just work.
        // Linux/X11: same. Linux/Wayland: we explicitly mark unsupported in
        // `spawn`, but there is no per-process trust bit to consult; return
        // `false` only when we know we cannot bind at all.
        #[cfg(target_os = "linux")]
        {
            if is_wayland() {
                return false;
            }
        }
        true
    }

    #[cfg(target_os = "linux")]
    fn is_wayland() -> bool {
        std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
    }

    /// Map config key names (shared with macOS) to `rdev::Key` variants.
    /// The default (and the only canonical TurboTalk key) is `right_option`,
    /// which on Windows/Linux is the Right Alt key — `rdev::Key::AltGr`.
    fn key_for_name(name: &str) -> rdev::Key {
        match name {
            "right_control" => rdev::Key::ControlRight,
            "right_command" => rdev::Key::MetaRight,
            "right_shift" => rdev::Key::ShiftRight,
            // "right_option" or anything unknown → Right Alt (AltGr on Win/X11).
            _ => rdev::Key::AltGr,
        }
    }

    pub fn spawn(
        recorder: Arc<Recorder>,
        tray_icon: TrayIcon,
        app: AppHandle,
        hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
    ) {
        // Wayland fast-fail: emit a clear ui-error and return without binding.
        #[cfg(target_os = "linux")]
        {
            if is_wayland() {
                let app_for_emit = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                    common::emit_critical(
                        &app_for_emit,
                        "ui-error",
                        common::UiError {
                            kind: "hotkey-unsupported",
                            message: "Push-to-talk on Linux requires X11. \
                                Wayland sessions are not supported. Log out and pick \
                                an X11 / Xorg session from your display manager."
                                .to_string(),
                            recoverable: false,
                        },
                    );
                });
                tracing::error!(
                    "[hotkey] Wayland session detected — global push-to-talk hook \
                    is not available on Wayland."
                );
                return;
            }
        }

        // Clone an app handle for the failure path before moving `app` into
        // the listener closure.
        let app_for_error = app.clone();

        std::thread::spawn(move || {
            // Track the current logical hotkey state so we don't double-fire on
            // OS auto-repeat (Windows in particular re-emits KeyPress while the
            // key is held). `rdev` does not deduplicate.
            let down = Arc::new(AtomicBool::new(false));

            // The `rdev::listen` callback is `Fn + 'static + Send`, owns
            // everything it captures, and runs on the listener thread. All
            // heavy work is delegated to `common::ptt_down` / `ptt_up`, which
            // already spawn worker threads internally.
            let recorder = recorder;
            let tray_icon = tray_icon;
            let app = app;
            let hotkey_state = hotkey_state;
            let down_for_cb = down.clone();
            let app_for_error = app_for_error;

            let result = rdev::listen(move |event: rdev::Event| {
                // Read current config under RwLock — nanoseconds, uncontended.
                let (target_key, toggle_mode, cancel_on_esc) = {
                    let hk = hotkey_state.read();
                    (
                        key_for_name(&hk.key),
                        hk.mode == "toggle",
                        hk.cancel_on_esc,
                    )
                };

                // Escape → cancel any in-flight recording. We act only when
                // the recorder is busy so Escape stays a no-op for the focused
                // app at idle.
                if let rdev::EventType::KeyPress(rdev::Key::Escape) = event.event_type {
                    if cancel_on_esc {
                        let s = recorder.state();
                        if matches!(
                            s,
                            crate::recorder::State::Recording
                                | crate::recorder::State::Transcribing
                        ) {
                            if !toggle_mode
                                && matches!(s, crate::recorder::State::Recording)
                            {
                                common::arm_ptt_up_suppression();
                            }
                            common::trigger_cancel(&recorder, &tray_icon, &app);
                            return;
                        }
                    }
                }

                match event.event_type {
                    rdev::EventType::KeyPress(key) if key == target_key => {
                        // De-dup OS auto-repeat: only act on the *transition*
                        // from up→down. Subsequent KeyPress events while held
                        // are ignored.
                        let was_down = down_for_cb.swap(true, Ordering::AcqRel);
                        if was_down {
                            return;
                        }
                        // Tap-cancel: two record-key down strokes within
                        // `TAP_CANCEL_WINDOW`, while a recording is in flight,
                        // abort the recording.
                        // Tap-cancel: toggle-mode only, always on (safety net
                        // for accidental double-toggles). Hold mode releases
                        // already stop the recording cleanly so it isn't
                        // needed there.
                        if toggle_mode {
                            let now = std::time::Instant::now();
                            let mut last = common::LAST_RECORD_KEY_DOWN.lock();
                            let recent = last
                                .map(|t| now.duration_since(t) < common::TAP_CANCEL_WINDOW)
                                .unwrap_or(false);
                            *last = Some(now);
                            drop(last);
                            if recent {
                                // First "recent" tap during a busy recorder
                                // cancels; subsequent taps in the burst extend
                                // the cooldown so 5–6 anxious mashes can't
                                // slip past into a fresh recording. Either
                                // branch suppresses normal PTT dispatch for
                                // this stroke. Toggle-mode releases are
                                // no-ops, so no ptt_up suppression needed.
                                let s = recorder.state();
                                let do_cancel = matches!(
                                    s,
                                    crate::recorder::State::Recording
                                        | crate::recorder::State::Transcribing
                                );
                                if do_cancel {
                                    common::trigger_tap_cancel(&recorder, &tray_icon, &app);
                                } else {
                                    common::mark_tap_cancel();
                                }
                                // Reset the down-tracking flag so the matching
                                // release isn't read as a spurious key-up.
                                down_for_cb.store(false, Ordering::Release);
                                return;
                            }
                        }
                        if toggle_mode {
                            if recorder.is_recording() {
                                common::ptt_up(&recorder, &tray_icon, &app);
                            } else {
                                common::ptt_down(&recorder, &tray_icon, &app);
                            }
                        } else {
                            common::ptt_down(&recorder, &tray_icon, &app);
                        }
                    }
                    rdev::EventType::KeyRelease(key) if key == target_key => {
                        let was_down = down_for_cb.swap(false, Ordering::AcqRel);
                        if !was_down {
                            return;
                        }
                        if !toggle_mode {
                            common::ptt_up(&recorder, &tray_icon, &app);
                        }
                        // Toggle mode: KeyRelease is a no-op; toggling happens
                        // on every KeyPress.
                    }
                    _ => {}
                }
            });

            if let Err(e) = result {
                tracing::error!("[hotkey] rdev::listen failed: {:?}", e);
                let _ = down; // closure consumed `down_for_cb`; avoid unused warning
                // Surface to UI via the cloned-outside-thread `app_for_error`.
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                    common::emit_critical(
                        &app_for_error,
                        "ui-error",
                        common::UiError {
                            kind: "hotkey-bind-failed",
                            message:
                                "Push-to-talk hotkey could not be bound. Restart Turbo Talk and \
                                try again."
                                    .to_string(),
                            recoverable: true,
                        },
                    );
                });
            }
        });
    }
}

pub use imp::accessibility_trusted;
pub use imp::spawn;

/// Programmatically start a recording — same path as the physical PTT down stroke.
/// Safe to call from any thread; spawns its own worker internally.
pub fn trigger_start(
    recorder: &std::sync::Arc<crate::recorder::Recorder>,
    tray_icon: &tauri::tray::TrayIcon,
    app: &tauri::AppHandle,
) {
    common::ptt_down(recorder, tray_icon, app);
}

/// Programmatically stop a recording and kick off transcription — same path as
/// the physical PTT up stroke. Safe to call from any thread.
pub fn trigger_stop(
    recorder: &std::sync::Arc<crate::recorder::Recorder>,
    tray_icon: &tauri::tray::TrayIcon,
    app: &tauri::AppHandle,
) {
    common::ptt_up(recorder, tray_icon, app);
}

/// Cancel an in-flight recording from anywhere (tray click, IPC command, the
/// internal Esc / tap-to-cancel hotkey paths). Idempotent — a Ready recorder
/// is left alone. Safe to call from any thread.
///
/// Pure cancel: callers who know a matching `ptt_up` is on its way (hold
/// mode + key still held) should call `arm_ptt_up_suppression` first so the
/// upcoming `ptt_up` no-ops instead of cascading into `CANCEL_PENDING`.
pub fn trigger_cancel(
    recorder: &std::sync::Arc<crate::recorder::Recorder>,
    tray_icon: &tauri::tray::TrayIcon,
    app: &tauri::AppHandle,
) {
    common::trigger_cancel(recorder, tray_icon, app);
}

/// Arm one `ptt_up` suppression slot. Use before `trigger_cancel` when the
/// caller knows the matching key release will dispatch `ptt_up` (hold mode +
/// recorder currently in `Recording`). In toggle mode releases are no-ops, so
/// arming would just swallow the user's next intentional press-to-stop.
pub fn arm_ptt_up_suppression() {
    common::arm_ptt_up_suppression();
}
