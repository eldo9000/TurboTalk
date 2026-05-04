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
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use tauri::{tray::TrayIcon, AppHandle, Emitter};

    /// Set by `ptt_up` when `rec.stop()` fails because the recorder wasn't
    /// Recording yet (key-up thread won the scheduler over key-down thread in
    /// hold mode). `ptt_down` checks this immediately after `rec.start()` and
    /// cancels the recording instead of showing the overlay, preventing the
    /// "quick tap → overlay stuck forever" bug.
    static CANCEL_PENDING: AtomicBool = AtomicBool::new(false);

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
        });
    }

    pub(super) fn ptt_up(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
        let rec = recorder.clone();
        let tray = tray_icon.clone();
        let app = app.clone();
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
                            let final_text = crate::cleanup::process(&raw_text);
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
                vec![CGEventType::FlagsChanged],
                move |_proxy, etype, event| {
                    let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                    let flags = event.get_flags();

                    // Read current config (RwLock read — nanoseconds, uncontended)
                    let (target_keycode, target_flag, toggle_mode) = {
                        let hk = hotkey_state.read();
                        let (kc, f) = key_for_name(&hk.key);
                        (kc, f, hk.mode == "toggle")
                    };

                    if let CGEventType::FlagsChanged = etype {
                        if keycode == target_keycode {
                            let is_key_down = flags.contains(target_flag);
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
                let (target_key, toggle_mode) = {
                    let hk = hotkey_state.read();
                    (key_for_name(&hk.key), hk.mode == "toggle")
                };

                match event.event_type {
                    rdev::EventType::KeyPress(key) if key == target_key => {
                        // De-dup OS auto-repeat: only act on the *transition*
                        // from up→down. Subsequent KeyPress events while held
                        // are ignored.
                        let was_down = down_for_cb.swap(true, Ordering::AcqRel);
                        if was_down {
                            return;
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
