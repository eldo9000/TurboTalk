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
    /// swallowed. The cancel paths (Esc, hold-to-cancel, IPC, tray) increment
    /// this so the matching key-up no-ops instead of cascading into
    /// IllegalTransition / CANCEL_PENDING. Counter, not flag, in case multiple
    /// cancel signals stack before the user releases.
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

    /// Segment transcriber started in `ptt_down` immediately after recording
    /// begins. Processes mid-recording segment cuts concurrently while the user
    /// is still speaking so that by key-release only the tail remains.
    /// Taken and used in `ptt_up`; taken and dropped (joining the worker) in
    /// cancel paths so the thread is never leaked.
    static CURRENT_SEG_TRANSCRIBER: parking_lot::Mutex<
        Option<crate::transcribe::SegmentTranscriber>,
    > = parking_lot::Mutex::new(None);

    /// Hold-to-cancel: how long the trigger key must be held during a busy
    /// recorder (Recording or Transcribing) before the gesture fires. Longer
    /// than typical tap-to-toggle latency so a normal toggle stop never trips
    /// the cancel.
    pub(super) const HOLD_CANCEL_DURATION: std::time::Duration =
        std::time::Duration::from_millis(500);

    /// Generation counter for hold-to-cancel candidate presses. Each qualifying
    /// trigger-key down stroke increments this; the timer thread captures its
    /// generation at arm time and aborts at the deadline if the value has
    /// since moved (newer press, or release-then-press).
    static HOLD_CANCEL_GEN: AtomicU64 = AtomicU64::new(0);

    /// True between trigger-key down and up for the most recent press, when
    /// that press armed a hold-to-cancel timer. Cleared on release so the
    /// timer's deadline check sees "no longer held" and bails.
    static HOLD_CANCEL_KEY_DOWN: AtomicBool = AtomicBool::new(false);

    /// Arm a hold-to-cancel timer if the recorder is in a cancellable state.
    /// Call from the OS listener thread on trigger-key down (after auto-repeat
    /// dedup). When the deadline elapses, if the same press is still held and
    /// the recorder is still Recording or Transcribing, fire `trigger_cancel`.
    ///
    /// `toggle_mode` controls whether `SUPPRESS_PTT_UP_COUNT` is armed: in
    /// hold mode a key-up `ptt_up` is always coming and needs the suppression
    /// slot; in toggle mode key-release is a no-op, so no suppression is
    /// needed and arming one would swallow the user's very next stop-press.
    pub(super) fn arm_hold_cancel(
        recorder: &Arc<Recorder>,
        tray_icon: &TrayIcon,
        app: &AppHandle,
        toggle_mode: bool,
    ) {
        let s = recorder.state();
        if !matches!(
            s,
            crate::recorder::State::Recording | crate::recorder::State::Transcribing
        ) {
            return;
        }
        let gen = HOLD_CANCEL_GEN.fetch_add(1, Ordering::AcqRel) + 1;
        HOLD_CANCEL_KEY_DOWN.store(true, Ordering::Release);
        let rec = recorder.clone();
        let tray = tray_icon.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(HOLD_CANCEL_DURATION);
            if HOLD_CANCEL_GEN.load(Ordering::Acquire) != gen {
                return;
            }
            if !HOLD_CANCEL_KEY_DOWN.load(Ordering::Acquire) {
                return;
            }
            if !matches!(
                rec.state(),
                crate::recorder::State::Recording | crate::recorder::State::Transcribing
            ) {
                return;
            }
            // In hold mode a key-up ptt_up is always on its way and needs
            // this slot. In toggle mode key-release is a no-op — skip so
            // the slot doesn't leak into the next recording's stop-press.
            if !toggle_mode {
                arm_ptt_up_suppression();
            }
            trigger_cancel(&rec, &tray, &app);
        });
    }

    /// Disarm any in-flight hold-to-cancel timer. Call from the listener on
    /// trigger-key up. Cheap — just clears the held flag; the timer thread
    /// observes it at the deadline.
    pub(super) fn disarm_hold_cancel() {
        HOLD_CANCEL_KEY_DOWN.store(false, Ordering::Release);
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
    pub(super) fn trigger_cancel(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
        if matches!(recorder.state(), crate::recorder::State::Ready) {
            return;
        }
        play_chime(ChimeEvent::Cancel);
        let rec = recorder.clone();
        let tray = tray_icon.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            rec.cancel();
            // Detach the segment transcriber (JoinHandle drop = detach, not
            // join). The worker will exit on its own once the segment channel
            // closes; we don't need its results.
            let _ = CURRENT_SEG_TRANSCRIBER.lock().take();
            let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
            emit_critical(&app, "recording-cancelled", ());
            // If cancel killed the whisper-server (Transcribing → Ready path),
            // the worker is now invalidated and READY is false. Re-warm so the
            // next PTT press doesn't sit on the yellow tile waiting for a server
            // that nobody restarted.
            if !crate::transcribe::is_ready() {
                crate::transcribe::prewarm(crate::settings::load(), app.clone());
            }
        });
    }

    /// Audio cue events. Each maps to one of the four `sound_on_*` config
    /// toggles and a distinct macOS system sound, so the user can keep them
    /// individually on/off and tell them apart by ear.
    #[derive(Clone, Copy)]
    pub(super) enum ChimeEvent {
        Start,
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
            ChimeEvent::Start => (cfg.sound_on_start, "/System/Library/Sounds/Pop.aiff"),
            ChimeEvent::Finish => (cfg.sound_on_finish, "/System/Library/Sounds/Bottle.aiff"),
            ChimeEvent::Cancel => (cfg.sound_on_cancel, "/System/Library/Sounds/Tink.aiff"),
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

            // Permanent prewarm failure — short-circuit instead of polling
            // for 30 s. Surface the pre-existing failure so the overlay
            // doesn't sit on the yellow tile waiting for a model that will
            // never load this session.
            if crate::transcribe::prewarm_failed() {
                tracing::warn!("[hotkey] start ignored — whisper prewarm failed earlier");
                emit_critical(
                    &app,
                    "ptt-arm-failed",
                    "Dictation model failed to load. Check Settings.".to_string(),
                );
                return;
            }

            // Two-stage arm: when whisper-server is still loading, show the
            // yellow "armed" tile (border only, no internals) until the model
            // is ready. The audio stream is opened only after readiness so a
            // user looking at the empty tile knows not to speak yet — the
            // tile only fills (red border, canvas, word pills) once we're
            // actually capturing.
            if !crate::transcribe::is_ready() {
                crate::transcribe::prewarm(crate::settings::load(), app.clone());

                // Pin the overlay to the cursor's monitor up front so the
                // arming tile never flashes on the wrong display.
                crate::reposition_overlay_to_cursor_monitor(&app);
                emit_critical(&app, "ptt-armed", ());
                tracing::info!("[hotkey] arming — waiting for whisper-server readiness");

                // Poll up to 30 s (matches whisper-server readiness budget
                // in TranscriptionWorker::from_config). 50 ms tick gives
                // sub-frame latency once READY flips. Bail early on:
                //   - CANCEL_PENDING (user released key during the wait)
                //   - PREWARM_FAILED (background thread reported failure)
                //   - device_lost (mic disappeared mid-wait)
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                let mut ready = false;
                let mut cancelled = false;
                loop {
                    if crate::transcribe::is_ready() {
                        ready = true;
                        break;
                    }
                    if crate::transcribe::prewarm_failed() {
                        break;
                    }
                    if CANCEL_PENDING.swap(false, Ordering::AcqRel) {
                        cancelled = true;
                        break;
                    }
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }

                if cancelled {
                    tracing::info!("[hotkey] arm cancelled — user released key during wait");
                    emit_critical(&app, "recording-cancelled", ());
                    return;
                }
                if !ready {
                    tracing::warn!("[hotkey] arm timed out waiting for whisper-server");
                    emit_critical(
                        &app,
                        "ptt-arm-failed",
                        "Dictation model didn't load in time.".to_string(),
                    );
                    return;
                }
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
                emit_critical(&app, "recording-cancelled", ());
                return;
            }

            // Start the segment transcriber now so concurrent mid-recording
            // segment transcriptions can run while the user is still speaking.
            // The receiver was installed in AudioCapture during start(); taking
            // it here transfers ownership to SegmentTranscriber.
            if let Some(seg_rx) = rec.take_segment_receiver() {
                *CURRENT_SEG_TRANSCRIBER.lock() =
                    Some(crate::transcribe::SegmentTranscriber::start(seg_rx));
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
            // Pin the overlay window to the cursor's monitor *before* emitting
            // ptt-down so the recording UI never flashes on the wrong display.
            // The arming branch above already repositioned, but a second call
            // is harmless (window position is set unconditionally).
            crate::reposition_overlay_to_cursor_monitor(&app);
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
                let _ = CURRENT_SEG_TRANSCRIBER.lock().take();
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
            // Recover the segment transcriber started at key-down. Used in the
            // Wav arm to assemble concurrent segment results with the tail.
            // Dropped automatically (joining its worker) in Discard / Err arms.
            let seg_transcriber_opt = CURRENT_SEG_TRANSCRIBER.lock().take();

            // Tray-state policy: Recording icon only during literal capture; the
            // moment we enter FinalizingAudio (inside `rec.stop()`) the tray flips
            // to Transcribing and stays that way through Cleaning + Pasting.
            // Idle is restored exactly once at the end of the lifecycle.
            match rec.stop() {
                Ok(StopOutcome::Wav { path, speech_detected }) => {
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
                        // Must emit recording-discarded so the frontend clears
                        // transcribing=true (set by ptt-up above). Without this
                        // the overlay hangs until the next ptt-down resets it.
                        emit_critical(&app, "recording-discarded", ());
                        if let Some(job_id) = job_id_opt {
                            emit_stage(&app, job_id, "ready");
                        }
                        return;
                    }
                    if let Some(job_id) = job_id_opt {
                        emit_stage(&app, job_id, "transcribing");
                    }

                    // Stage 1: transcribe the tail WAV (audio after the last
                    // segment cut, or the whole recording if no segments were
                    // emitted — identical to pre-TASK-54 batch behavior).
                    // Skip Whisper when VAD detected no speech: Whisper
                    // hallucinating on silence is worse than an empty result.
                    let tail_result = if speech_detected {
                        crate::transcribe::run_raw(&path)
                    } else {
                        tracing::info!(
                            "[hotkey job_id={:?}] tail speech_detected=false — skipping Whisper",
                            job_id_opt
                        );
                        Ok(String::new())
                    };

                    // Stage 2: wait for any in-flight concurrent segment
                    // transcriptions (started at key-down) to finish, then
                    // assemble them in emission order. Segments precede the
                    // tail chronologically.
                    let seg_text = seg_transcriber_opt
                        .map(|st| st.join_segments())
                        .unwrap_or_default();

                    let transcribe_result: anyhow::Result<String> = match tail_result {
                        Ok(tail) => {
                            let parts: Vec<&str> = [seg_text.as_str(), tail.as_str()]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .collect();
                            Ok(parts.join(" "))
                        }
                        Err(e) => {
                            if !seg_text.is_empty() {
                                tracing::warn!(
                                    "[transcribe job_id={:?}] tail failed, \
                                     using {} chars from segments: {}",
                                    job_id_opt,
                                    seg_text.chars().count(),
                                    e
                                );
                                Ok(seg_text)
                            } else {
                                Err(e)
                            }
                        }
                    };

                    // Transcribing → Cleaning (always — even on whisper error,
                    // so the lifecycle reaches `finish` through legal transitions).
                    if rec.begin_cleaning().is_err() {
                        // Should be unreachable given begin_transcribing succeeded,
                        // but if it does happen the recorder has been forced out
                        // from under us (e.g. cancel). Bail cleanly.
                        rec.finish();
                        let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                        emit_critical(&app, "recording-discarded", ());
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
                            if final_text.is_empty() {
                                tracing::info!(
                                    "[cleanup   job_id={:?}] empty final transcript — skipping paste",
                                    job_id_opt
                                );
                                emit_critical(&app, "recording-discarded", "empty-final-text");
                                rec.finish();
                                let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                                if let Some(job_id) = job_id_opt {
                                    emit_stage(&app, job_id, "ready");
                                }
                                play_chime(ChimeEvent::Finish);
                                return;
                            }
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
                            let paste_text = format!("{} ", final_text);
                            if let Err(e) = crate::paste::paste(&paste_text) {
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
                    // Tail-empty recovery: when a silence-boundary segment cut
                    // takes all the audio just before stop(), the tail has 0
                    // samples and stop() returns TooShort. The segment
                    // transcription is still in flight — join it and, if it
                    // produced text, treat it as the final result rather than
                    // silently discarding a full recording.
                    if matches!(reason, DiscardReason::TooShort { .. }) {
                        if let Some(st) = seg_transcriber_opt {
                            // Emit ptt-up NOW, before blocking on join_segments(),
                            // so the overlay transitions to "Transcribing…"
                            // immediately. Without this the overlay stays stuck in
                            // "Recording" for the entire Whisper inference window
                            // (1–3 s), making the user think recording is still live.
                            let _ = tray.set_icon(Some(tray::make_icon(TrayState::Transcribing)));
                            emit_critical(&app, "ptt-up", ());
                            if let Some(job_id) = job_id_opt {
                                emit_stage(&app, job_id, "transcribing");
                            }

                            let seg_text = st.join_segments(); // blocks; channel already closed by stop()

                            // If the user pressed record again while we were
                            // blocked, ptt-down already set the overlay back to
                            // "Recording". Emitting transcript now would clear
                            // recording=true and corrupt the new job's UI state.
                            // Abandon the recovery silently — the new job takes
                            // priority and the user explicitly started it.
                            if CURRENT_JOB_ID.lock().is_some() {
                                tracing::warn!(
                                    "[hotkey job_id={:?}] seg-recovery: new job started during \
                                     join_segments() — abandoning recovery to protect new job's UI",
                                    job_id_opt
                                );
                                return;
                            }

                            if !seg_text.is_empty() {
                                let final_text = crate::cleanup::process(&seg_text, &app);
                                if final_text.is_empty() {
                                    emit_critical(&app, "recording-discarded", "empty-final-text");
                                    play_chime(ChimeEvent::Finish);
                                } else {
                                    emit_critical(&app, "transcript", final_text.clone());
                                    if let Some(job_id) = job_id_opt {
                                        emit_stage(&app, job_id, "pasting");
                                    }
                                    let focus_at_paste = crate::paste::frontmost_app();
                                    tracing::info!(
                                        "[paste job_id={:?}] (seg-recovery) focus_at_start={:?} focus_at_paste={:?}",
                                        job_id_opt, focus_at_start, focus_at_paste
                                    );
                                    if let (Some(job_id), Some(start), Some(now)) =
                                        (job_id_opt, focus_at_start.as_ref(), focus_at_paste.as_ref())
                                    {
                                        if start != now {
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
                                    let paste_text = format!("{} ", final_text);
                                    if let Err(e) = crate::paste::paste(&paste_text) {
                                        tracing::error!("[paste job_id={:?}] (seg-recovery) {:?}", job_id_opt, e);
                                        emit_critical(&app, "paste-error", "Couldn't paste — check Accessibility permission".to_string());
                                    } else {
                                        play_chime(ChimeEvent::Finish);
                                    }
                                }
                            } else {
                                // Segments produced no text — normal discard.
                                emit_critical(&app, "recording-discarded", ());
                            }
                            let _ = tray.set_icon(Some(tray::make_icon(TrayState::Idle)));
                            if let Some(job_id) = job_id_opt {
                                emit_stage(&app, job_id, "ready");
                            }
                            return;
                        }
                    }
                    // Normal discard path — `recording-discarded` is the catch-all
                    // the overlay listens to; `recording-too-short` is the more
                    // specific subtype the main window uses to show a toast.
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
            // Permission watchdog loop. CGEventTap::new fails if the process
            // lacks Accessibility trust at this moment. Pre-fix behaviour was
            // to give up and require quit+relaunch — terrible first-run UX.
            // Now: emit the toast once, poll AXIsProcessTrusted() every
            // 1.5 s, and rebuild the tap as soon as trust flips. The fresh
            // closure clones happen each iteration because CGEventTap::new
            // consumes the callback regardless of outcome.
            let mut surfaced_permission_error = false;
            // Cap "trusted but tap creation fails" retries so a real OS-level
            // error (HID disabled, sandbox kill, …) doesn't burn CPU forever.
            let mut trusted_failure_retries = 0u32;
            const MAX_TRUSTED_FAILURE_RETRIES: u32 = 6; // ~30 s of 5 s sleeps

            loop {
                let recorder_cb = recorder.clone();
                let tray_cb = tray_icon.clone();
                let app_cb = app.clone();
                let hk_cb = hotkey_state.clone();

                let tap_result = CGEventTap::new(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
                    move |_proxy, etype, event| {
                        let keycode =
                            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                        let flags = event.get_flags();

                        // Read current config (RwLock read — nanoseconds, uncontended)
                        let (
                            target_keycode,
                            target_flag,
                            toggle_mode,
                            cancel_on_esc,
                            cancel_on_hold,
                        ) = {
                            let hk = hk_cb.read();
                            let (kc, f) = key_for_name(&hk.key);
                            (
                                kc,
                                f,
                                hk.mode == "toggle",
                                hk.cancel_on_esc,
                                hk.cancel_on_hold,
                            )
                        };

                        // Escape → cancel any in-flight recording. Read-only on
                        // events outside Recording/Transcribing so it never
                        // swallows Escape from the focused app while idle.
                        if let CGEventType::KeyDown = etype {
                            if cancel_on_esc && keycode == ESCAPE_KEYCODE {
                                let s = recorder_cb.state();
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
                                    common::trigger_cancel(&recorder_cb, &tray_cb, &app_cb);
                                }
                            }
                        }

                        if let CGEventType::FlagsChanged = etype {
                            if keycode == target_keycode {
                                let is_key_down = flags.contains(target_flag);
                                // Hold-to-cancel: any trigger-key down stroke
                                // while the recorder is busy (Recording or
                                // Transcribing) arms a 500 ms timer. If the
                                // user keeps holding past the deadline, the
                                // in-flight job is cancelled. Released early
                                // → timer no-ops, normal PTT semantics apply.
                                if is_key_down {
                                    if cancel_on_hold {
                                        common::arm_hold_cancel(&recorder_cb, &tray_cb, &app_cb, toggle_mode);
                                    }
                                } else {
                                    common::disarm_hold_cancel();
                                }
                                if toggle_mode {
                                    if is_key_down {
                                        if recorder_cb.is_recording() {
                                            common::ptt_up(&recorder_cb, &tray_cb, &app_cb);
                                        } else {
                                            common::ptt_down(&recorder_cb, &tray_cb, &app_cb);
                                        }
                                    }
                                } else if is_key_down {
                                    common::ptt_down(&recorder_cb, &tray_cb, &app_cb);
                                } else {
                                    common::ptt_up(&recorder_cb, &tray_cb, &app_cb);
                                }
                            }
                        }

                        None
                    },
                );

                match tap_result {
                    Ok(tap) => {
                        if surfaced_permission_error {
                            tracing::info!(
                                "[hotkey] CGEventTap rebuilt after Accessibility permission grant"
                            );
                        }
                        let source = tap
                            .mach_port
                            .create_runloop_source(0)
                            .expect("[hotkey] create_runloop_source failed");
                        // SAFETY: kCFRunLoopCommonModes is a static CFStringRef
                        // constant exported by core-foundation. Reading it requires
                        // unsafe because the binding is a static extern, but the
                        // value is immutable and thread-safe to read.
                        CFRunLoop::get_current()
                            .add_source(&source, unsafe { kCFRunLoopCommonModes });
                        tap.enable();
                        CFRunLoop::run_current();
                        return;
                    }
                    Err(()) => {
                        let trusted = accessibility_trusted();
                        tracing::error!(
                            "[hotkey] CGEventTap failed (accessibility_trusted={trusted}, retry={trusted_failure_retries})"
                        );
                        if !surfaced_permission_error {
                            surfaced_permission_error = true;
                            let (kind, message) = if trusted {
                                (
                                    "hotkey-input-monitoring",
                                    "Record trigger could not receive keyboard events. Turn on Turbo Talk in System Settings → Privacy & Security → Input Monitoring, then restart Turbo Talk.",
                                )
                            } else {
                                (
                                    "hotkey-permission",
                                    "Record trigger needs Accessibility permission. Add Turbo Talk in System Settings → Privacy & Security → Accessibility — Turbo Talk will pick it up automatically once granted.",
                                )
                            };
                            let app_for_emit = app.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(1200));
                                common::emit_critical(
                                    &app_for_emit,
                                    "ui-error",
                                    common::UiError {
                                        kind,
                                        message: message.to_string(),
                                        recoverable: true,
                                    },
                                );
                            });
                        }
                        if !trusted {
                            // Permission missing. Poll cheaply until it flips.
                            // AXIsProcessTrusted() is a fast syscall; 1.5 s is a
                            // human-scale latency for "I just clicked the toggle".
                            loop {
                                std::thread::sleep(std::time::Duration::from_millis(1500));
                                if accessibility_trusted() {
                                    tracing::info!(
                                        "[hotkey] Accessibility permission detected — retrying CGEventTap"
                                    );
                                    // Reset the trusted-failure budget for the rebuild attempt.
                                    trusted_failure_retries = 0;
                                    break;
                                }
                            }
                        } else {
                            // Trusted but the tap still failed — real OS-level
                            // problem. Back off and cap retries so we don't spin
                            // forever on something the user can't fix.
                            trusted_failure_retries += 1;
                            if trusted_failure_retries >= MAX_TRUSTED_FAILURE_RETRIES {
                                tracing::error!(
                                    "[hotkey] giving up after {trusted_failure_retries} trusted-but-failing retries"
                                );
                                return;
                            }
                            std::thread::sleep(std::time::Duration::from_secs(5));
                        }
                    }
                }
            }
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
                let (target_key, toggle_mode, cancel_on_esc, cancel_on_hold) = {
                    let hk = hotkey_state.read();
                    (
                        key_for_name(&hk.key),
                        hk.mode == "toggle",
                        hk.cancel_on_esc,
                        hk.cancel_on_hold,
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
                            if !toggle_mode && matches!(s, crate::recorder::State::Recording) {
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
                        // Hold-to-cancel: arm a 500 ms timer if the recorder
                        // is busy. Held past the deadline → cancel; released
                        // early → no-op and normal PTT semantics apply.
                        if cancel_on_hold {
                            common::arm_hold_cancel(&recorder, &tray_icon, &app, toggle_mode);
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
                        common::disarm_hold_cancel();
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
