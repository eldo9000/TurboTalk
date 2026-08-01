// Push-to-talk hotkey binding.
//
// Architecture (three layers):
//   1. Platform facade (`mod imp`) — OS-specific key-event source.
//   2. Controller layer (`Controller` / `HoldController` / `ToggleController`)
//      — owns mode-specific lifecycle rules (hold vs toggle).
//   3. Shared dictation engine (`mod common`) — `ptt_down`, `ptt_up`,
//      cancel, paste, chimes, focus tracking, segment recovery.
//
// Each per-OS `mod imp` constructs a `Controller` once per event and calls
// `.press()` / `.release()` — no mode-specific branching in platform code.
//
// Public surface preserved by every branch:
//   - `pub fn spawn(recorder, tray_icon, app, hotkey_state)` — the only
//     entry point `lib.rs::run` calls into.
//   - `pub fn accessibility_trusted() -> bool` — used by the onboarding
//     readiness gate.

pub(crate) mod common {
    use crate::audio::{DiscardReason, StopOutcome};
    use crate::recorder::{Recorder, RecorderError};
    use crate::session_metrics;
    use crate::tray::{self, TrayState};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::Arc;
    use tauri::{tray::TrayIcon, AppHandle, Emitter, Manager};

    /// Set by `ptt_up` when `rec.stop()` fails because the recorder wasn't
    /// Recording yet (key-up thread won the scheduler over key-down thread in
    /// hold mode). `ptt_down` checks this immediately after `rec.start()` and
    /// cancels the recording instead of showing the overlay, preventing the
    /// "quick tap → overlay stuck forever" bug.
    static CANCEL_PENDING: AtomicBool = AtomicBool::new(false);

    /// Epoch millis when the current recording was accepted. Used as a short
    /// toggle-mode debounce so duplicate HID/CG paths cannot immediately turn
    /// one physical press into start→stop.
    pub(super) static LAST_RECORDING_START_MS: AtomicU64 = AtomicU64::new(0);

    /// True while a key-down worker is arming or starting a recording. A quick
    /// hold-mode key-up may arrive while this is true and request cancellation;
    /// a stray key-up while idle must not poison the next start.
    static START_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

    /// Set by a second `ptt_down` press arriving during toggle-mode arming
    /// (while `START_IN_FLIGHT` is true and prewarm is in flight). The poll
    /// loop reads this to cancel the arming tile. Hold mode cancels via
    /// key-up → `CANCEL_PENDING`; toggle has no key-up, so this flag gives
    /// the user an explicit "press again during warmup = cancel" path.
    /// Set by ToggleController when the user presses again during warmup.
    /// Read by the shared engine's polling loop (`ptt_down`) to cancel arming.
    /// Hold mode uses key-up → CANCEL_PENDING instead.
    pub(super) static CANCEL_ARMING: AtomicBool = AtomicBool::new(false);

    struct StartInFlightGuard;

    impl Drop for StartInFlightGuard {
        fn drop(&mut self) {
            START_IN_FLIGHT.store(false, Ordering::Release);
        }
    }

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

    /// Incremented on every accepted cancel action. The ptt-up worker captures
    /// this before `rec.stop()` (see `cancel_epoch_at_stop`), then checks it
    /// before emitting or pasting text.  SeqCst is used so the increment from
    /// the CGEventTap callback is globally visible to the ptt_up worker thread
    /// without relying on inter-thread cache-coherence timing.
    static CANCEL_EPOCH: AtomicU64 = AtomicU64::new(0);

    fn cancel_epoch() -> u64 {
        CANCEL_EPOCH.load(Ordering::SeqCst)
    }

    pub(super) fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn job_cancelled_since(epoch: u64) -> bool {
        CANCEL_EPOCH.load(Ordering::SeqCst) != epoch
    }

    fn wait_for_hold_cancel_window(epoch: u64, job_id_opt: Option<u64>) -> bool {
        if job_cancelled_since(epoch) {
            return true;
        }
        if !HOLD_CANCEL_KEY_DOWN.load(Ordering::Acquire) {
            return false;
        }

        let deadline = std::time::Instant::now() + HOLD_CANCEL_DURATION;
        tracing::debug!(
            "[hotkey job_id={:?}] paste waiting for active hold-cancel window",
            job_id_opt
        );
        while HOLD_CANCEL_KEY_DOWN.load(Ordering::Acquire) {
            if job_cancelled_since(epoch) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        job_cancelled_since(epoch)
    }

    /// Payload for the additive `dictation-stage` event.
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

    /// Payload for the additive `focus-changed-before-paste` event.
    /// Emitted only when the frontmost-app identifier captured at
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
        crate::diagnostic_log::emergency_trace(format!("[emit] {event}"));
        if let Err(e) = app.emit(event, payload) {
            crate::diagnostic_log::emergency_trace(format!("[emit-failed] {event} {e:?}"));
            tracing::warn!("[hotkey] failed to emit {}: {:?}", event, e);
        }
    }

    /// Show the main TurboTalk window and switch to the History tab.
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
    /// recorder before the gesture fires. Longer than typical tap-to-toggle
    /// latency so a normal toggle stop never trips the cancel.
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

    /// True when a newly pressed trigger key should arm hold-to-cancel.
    /// The start press itself is intentionally excluded: a user who holds the
    /// key while a toggle recording is warming up/starting should not
    /// accidentally cancel the recording they just began.
    pub(super) fn should_arm_hold_cancel(recorder: &Arc<Recorder>) -> bool {
        recorder.state().is_busy()
    }

    /// Arm a hold-to-cancel timer. When the deadline elapses, if the same
    /// press is still held and the recorder is still busy, fire
    /// `trigger_cancel`.
    ///
    /// Arm a hold-to-cancel timer. When the deadline elapses, if the same
    /// press is still held and the recorder is still busy, fire
    /// `trigger_cancel`.
    ///
    /// Suppression (`SUPPRESS_PTT_UP_COUNT`) is NOT armed here — the
    /// controller (HoldController vs ToggleController) handles that before
    /// calling this function.
    pub(super) fn arm_hold_cancel(
        recorder: &Arc<Recorder>,
        tray_icon: &TrayIcon,
        app: &AppHandle,
    ) {
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
            if !rec.state().is_busy() {
                return;
            }
            // Suppression is handled by the controller before calling
            // arm_hold_cancel.
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
        CANCEL_EPOCH.fetch_add(1, Ordering::SeqCst);
        play_chime(app, ChimeEvent::Cancel);
        let rec = recorder.clone();
        let tray = tray_icon.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            if crate::settings::pause_media_on_dictate() {
                crate::media_control::resume();
            }
            rec.cancel();
            // Detach the segment transcriber (JoinHandle drop = detach, not
            // join). The worker will exit on its own once the segment channel
            // closes; we don't need its results.
            let _ = CURRENT_SEG_TRANSCRIBER.lock().take();
            tray::set_tray_icon(&tray, TrayState::Idle);
            emit_critical(&app, "recording-cancelled", ());
            // If cancel killed the whisper-server (Transcribing → Ready path),
            // the worker is now invalidated and READY is false. Re-warm so the
            // next PTT press doesn't sit on the yellow tile waiting for a server
            // that nobody restarted.
            if !crate::transcribe::is_ready() {
                crate::transcribe::prewarm(
                    (*crate::settings::load()).clone(),
                    app.clone(),
                );
            }
        });
    }

    /// Audio cue events. Each maps to one of the four `sound_on_*` config
    /// toggles and a distinct macOS system sound, so the user can keep them
    /// individually on/off and tell them apart by ear.
    #[derive(Debug, Clone, Copy)]
    pub(crate) enum ChimeEvent {
        Start,
        Finish,
        Cancel,
        Error,
    }

    /// Soft event chime. Played from the backend rather than the frontend
    /// because the overlay window has `focus: false` and ignores cursor
    /// events — WKWebView blocks `AudioContext` audio without a user
    /// gesture, so a frontend Web Audio chime would be silently dropped.
    /// Plays a system sound via `NSSound` on macOS (dispatched to the main
    /// thread — AppKit classes are unreliable from background threads),
    /// `MessageBeep` on Windows, no-op on Linux.
    pub(crate) fn play_chime(app: &AppHandle, event: ChimeEvent) {
        #[cfg(target_os = "macos")]
        {
            let cfg = crate::settings::load();
            let enabled = match event {
                ChimeEvent::Start => cfg.sound_on_start,
                ChimeEvent::Finish => cfg.sound_on_finish,
                ChimeEvent::Cancel => cfg.sound_on_cancel,
                ChimeEvent::Error => cfg.sound_on_error,
            };
            if !enabled {
                return;
            }

            let sound_name: &'static str = match event {
                ChimeEvent::Start => "Pop",
                ChimeEvent::Finish => "Bottle",
                ChimeEvent::Cancel => "Tink",
                ChimeEvent::Error => "Basso",
            };

            let app = app.clone();
            let _ = app.run_on_main_thread(move || {
                use objc2_app_kit::NSSound;
                use objc2_foundation::NSString;

                let ns_name = NSString::from_str(sound_name);
                if let Some(ns_sound) = NSSound::soundNamed(&ns_name) {
                    // The system cache holds its own permanent retain on
                    // system sounds — we don't need to keep the Retained
                    // alive; playback completes regardless of when our
                    // reference drops.
                    if ns_sound.play() {
                        tracing::info!("[chime] NSSound {}", sound_name);
                    } else {
                        tracing::warn!("[chime] NSSound play returned NO for '{}'", sound_name);
                    }
                } else {
                    tracing::warn!(
                        "[chime] NSSound soundNamed returned null for '{}'",
                        sound_name
                    );
                }
            });
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = app;
            let cfg = crate::settings::load();
            let (enabled, _sound) = match event {
            // Sound choices are deliberate: short and "soft" by design, and
            // distinct enough that the five events are recognizable by ear.
            //   Pop    — quick percussive "go" for recording start
            //   Morse  — two-blip "thinking" pattern while transcribing
            //   Bottle — gentle glass clink at paste time
            //   Tink   — softest of the system sounds, paired with cancel
            //   Basso  — low, obvious beep for errors / filtered dictation
            ChimeEvent::Start => (cfg.sound_on_start, "/System/Library/Sounds/Pop.aiff"),
            ChimeEvent::Finish => (cfg.sound_on_finish, "/System/Library/Sounds/Bottle.aiff"),
            ChimeEvent::Cancel => (cfg.sound_on_cancel, "/System/Library/Sounds/Tink.aiff"),
            ChimeEvent::Error => (cfg.sound_on_error, "/System/Library/Sounds/Basso.aiff"),
        };
        if !enabled {
            return;
        }
        #[cfg(target_os = "windows")]
        {
            use std::f32::consts::PI;
            use std::sync::OnceLock;

            const SAMPLE_RATE: u32 = 44100;

            #[link(name = "winmm")]
            extern "system" {
                fn PlaySoundW(
                    pszSound: *const std::ffi::c_void,
                    hmod: *mut std::ffi::c_void,
                    fdwSound: u32,
                );
            }
            const SND_ASYNC: u32 = 0x0001;
            const SND_MEMORY: u32 = 0x0004;
            const SND_NODEFAULT: u32 = 0x0002;

            /// A single oscillator: fixed or sweeping frequency.
            struct Partial {
                freq_start: f32,
                freq_end: f32,
                amp: f32,
            }

            fn make_wav(duration_secs: f32, attack_secs: f32, partials: &[Partial]) -> Vec<u8> {
                let num_samples = (SAMPLE_RATE as f32 * duration_secs) as usize;
                let data_size = num_samples as u32 * 2;
                let file_size = 36 + data_size;

                let mut wav = Vec::with_capacity(44 + data_size as usize);
                wav.extend(b"RIFF");
                wav.extend(&file_size.to_le_bytes());
                wav.extend(b"WAVE");
                wav.extend(b"fmt ");
                wav.extend(&16u32.to_le_bytes());
                wav.extend(&1u16.to_le_bytes());
                wav.extend(&1u16.to_le_bytes());
                wav.extend(&SAMPLE_RATE.to_le_bytes());
                wav.extend(&(SAMPLE_RATE * 2).to_le_bytes());
                wav.extend(&2u16.to_le_bytes());
                wav.extend(&16u16.to_le_bytes());
                wav.extend(b"data");
                wav.extend(&data_size.to_le_bytes());

                let decay_start = attack_secs / duration_secs;
                for i in 0..num_samples {
                    let t = i as f32 / SAMPLE_RATE as f32;
                    let pos = t / duration_secs;
                    let envelope = if pos < decay_start {
                        pos / decay_start
                    } else {
                        (-4.0 * (pos - decay_start) / (1.0 - decay_start)).exp()
                    };
                    let mut sample = 0.0;
                    for p in partials {
                        let freq = p.freq_start + (p.freq_end - p.freq_start) * pos;
                        sample += (2.0 * PI * freq * t).sin() * p.amp;
                    }
                    sample = (sample * envelope).clamp(-1.0, 1.0);
                    wav.extend(&((sample * i16::MAX as f32) as i16).to_le_bytes());
                }
                wav
            }

            fn get_wav(event: ChimeEvent) -> &'static [u8] {
                static POP: OnceLock<Vec<u8>> = OnceLock::new();
                static BOTTLE: OnceLock<Vec<u8>> = OnceLock::new();
                static TINK: OnceLock<Vec<u8>> = OnceLock::new();
                static BASSO: OnceLock<Vec<u8>> = OnceLock::new();
                match event {
                    // Pop — quick percussive click, mostly broad-spectrum transient.
                    // Short sine burst at ~700Hz with fast attack and sharp decay mimics
                    // the macOS "Pop" system sound (like a cork or finger snap).
                    ChimeEvent::Start => POP.get_or_init(|| {
                        make_wav(0.04, 0.001, &[
                            Partial { freq_start: 700.0, freq_end: 700.0, amp: 1.0 },
                            Partial { freq_start: 1400.0, freq_end: 1400.0, amp: 0.25 },
                        ])
                    }),
                    // Bottle — gentle glass clink. Uses inharmonic partials with
                    // faster high-frequency decay (higher partials have lower amp)
                    // to mimic the macOS "Bottle" system sound.
                    ChimeEvent::Finish => BOTTLE.get_or_init(|| {
                        make_wav(0.18, 0.002, &[
                            Partial { freq_start: 1100.0, freq_end: 1100.0, amp: 1.0 },
                            Partial { freq_start: 1870.0, freq_end: 1870.0, amp: 0.3 },
                            Partial { freq_start: 3080.0, freq_end: 3080.0, amp: 0.08 },
                        ])
                    }),
                    // Tink — very soft, delicate high chime. The macOS "Tink" is
                    // the quietest and most delicate of the four.
                    ChimeEvent::Cancel => TINK.get_or_init(|| {
                        make_wav(0.07, 0.002, &[
                            Partial { freq_start: 2200.0, freq_end: 2200.0, amp: 1.0 },
                            Partial { freq_start: 4400.0, freq_end: 4400.0, amp: 0.15 },
                        ])
                    }),
                    // Basso — low descending bass note. Sweeps from ~200Hz down to
                    // ~110Hz with a harmonic at 2x, mimicking the macOS "Basso"
                    // cartoon-like descending tone.
                    ChimeEvent::Error => BASSO.get_or_init(|| {
                        make_wav(0.35, 0.005, &[
                            Partial { freq_start: 200.0, freq_end: 110.0, amp: 1.0 },
                            Partial { freq_start: 400.0, freq_end: 220.0, amp: 0.25 },
                        ])
                    }),
                }
            }

            let wav_data = get_wav(event);
            unsafe {
                PlaySoundW(
                    wav_data.as_ptr() as *const std::ffi::c_void,
                    std::ptr::null_mut(),
                    SND_ASYNC | SND_MEMORY | SND_NODEFAULT,
                );
            }
            tracing::info!("[chime] PlaySoundW ({:?})", event);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (cfg, sound);
        }
    }
    }

    // All work that touches the audio pipeline must run off the listener thread.
    // On macOS the CGEventTap callback is timeout-bounded; on Windows/Linux the
    // `rdev::listen` callback runs on the listener thread and any blocking work
    // there would stall the global keyboard hook. Both ptt_down and ptt_up
    // therefore spawn a worker thread and return immediately.

    // Boost thread priority on macOS via QoS
    fn boost_thread() {
        #[cfg(target_os = "macos")]
        unsafe {
            // QOS_CLASS_USER_INITIATED = 0x19
            extern "C" {
                fn pthread_set_qos_class_self_np(class: i32, relative_priority: i32) -> i32;
            }
            let _ = pthread_set_qos_class_self_np(0x19, 0);
        }
    }

    pub(super) fn ptt_down(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
        crate::diagnostic_log::emergency_trace(format!(
            "[ptt_down] enter onboarding={} recorder={} ready={} prewarm_in_flight={} prewarm_failed={}",
            crate::permissions::onboarding_active(),
            recorder.state(),
            crate::transcribe::is_ready(),
            crate::transcribe::prewarm_in_flight(),
            crate::transcribe::prewarm_failed(),
        ));
        // Suppress all hotkey activity while the welcome/onboarding screen is
        // visible — no model, no permissions, no reason to arm.
        if crate::permissions::onboarding_active() {
            crate::diagnostic_log::emergency_trace("[ptt_down] ignored onboarding_active");
            return;
        }

        // Clear any stale CANCEL_PENDING / CANCEL_ARMING flags set by a
        // previous orphaned key-up or cancelled arming. Without
        // this the next legitimate press can instantly cancel itself.
        CANCEL_PENDING.store(false, Ordering::Relaxed);
        CANCEL_ARMING.store(false, Ordering::Relaxed);

        if START_IN_FLIGHT.swap(true, Ordering::AcqRel) {
            crate::diagnostic_log::emergency_trace("[ptt_down] ignored start_in_flight");
            tracing::debug!("[hotkey] start ignored — start already in flight");
            return;
        }
        let rec = recorder.clone();
        let tray = tray_icon.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            boost_thread();
            let _start_guard = StartInFlightGuard;
            // One-in-flight policy: only `Ready` is allowed to start a new job.
            // If the recorder is busy (anything from FinalizingAudio through
            // Pasting still running from a prior press), report it as
            // `dictation-busy` so the UI/user can observe the dropped press
            // without us silently swallowing it.
            let snapshot = rec.state();
            if snapshot.is_busy() {
                crate::diagnostic_log::emergency_trace(format!(
                    "[ptt_down] ignored recorder_busy state={snapshot}"
                ));
                tracing::warn!("[hotkey] start ignored — recorder busy in {}", snapshot);
                emit_critical(&app, "dictation-busy", snapshot.to_string());
                return;
            }

            // Permanent prewarm failure — short-circuit instead of polling
            // for 30 s. Surface the pre-existing failure so the overlay
            // doesn't sit on the yellow tile waiting for a model that will
            // never load this session.
            if crate::transcribe::prewarm_failed() {
                crate::diagnostic_log::emergency_trace("[ptt_down] ptt-arm-failed prewarm_failed");
                tracing::warn!("[hotkey] start ignored — whisper prewarm failed earlier");
                emit_critical(
                    &app,
                    "ptt-arm-failed",
                    "Dictation model failed to load. Check Settings.".to_string(),
                );
                return;
            }

            // Tracks whether the yellow "arming" tile has already been shown
            // this press (by the whisper-readiness branch below). The audio-
            // live gate after `rec.start()` reuses the same tile, so it only
            // emits `ptt-armed` if this is still false.
            let mut overlay_armed = false;

            // Two-stage arm: when whisper-server is still loading, show the
            // yellow "armed" tile (border only, no internals) until the model
            // is ready. The audio stream is opened only after readiness so a
            // user looking at the empty tile knows not to speak yet — the
            // tile only fills (red border, canvas, word pills) once we're
            // actually capturing.
            if !crate::transcribe::is_ready() {
                // If prewarm is already in flight from a prior press, this
                // press is interpreted as a cancel — dismiss the arming
                // overlay instead of queuing a second polling loop.
                if crate::transcribe::prewarm_in_flight() {
                    tracing::info!("[hotkey] arm cancelled — user pressed again during warmup");
                    emit_critical(&app, "recording-cancelled", ());
                    return;
                }

                crate::transcribe::prewarm(
                    (*crate::settings::load()).clone(),
                    app.clone(),
                );
                crate::diagnostic_log::emergency_trace("[ptt_down] prewarm started; emit ptt-armed");

                // Position the overlay before the frontend event so it doesn't
                // flash at a stale startup location.
                crate::windowing::reposition_overlay_to_cursor_monitor(&app);
                emit_critical(&app, "ptt-armed", ());
                overlay_armed = true;
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
                    if CANCEL_ARMING.swap(false, Ordering::AcqRel) {
                        cancelled = true;
                        break;
                    }
                    if std::time::Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }

                if cancelled {
                    crate::diagnostic_log::emergency_trace("[ptt_down] arming cancelled");
                    tracing::info!("[hotkey] arm cancelled — user released key during wait");
                    emit_critical(&app, "recording-cancelled", ());
                    return;
                }
                if !ready {
                    crate::diagnostic_log::emergency_trace(format!(
                        "[ptt_down] ptt-arm-failed wait_ready ready={} prewarm_failed={}",
                        ready,
                        crate::transcribe::prewarm_failed()
                    ));
                tracing::warn!("[hotkey] arm timed out waiting for whisper-server");
                emit_critical(
                        &app,
                    "ptt-arm-failed",
                    "Dictation model didn't load in time.".to_string(),
                    );
                    return;
                }
            }

            session_metrics::record_hotkey_down();

            // Pause media playback before capture starts.
            if crate::settings::pause_media_on_dictate() {
                crate::media_control::pause();
            }

            // Play the start chime BEFORE rec.start() so CoreAudio input setup
            // (which shares the audio session) doesn't silence the output sound.
            play_chime(&app, ChimeEvent::Start);

            if let Err(e) = rec.start() {
                crate::diagnostic_log::emergency_trace(format!(
                    "[ptt_down] rec.start failed state={} err={e}",
                    rec.state()
                ));
                // Race: state moved out of Ready between our snapshot and the
                // start() call (e.g. another press won the lock first), or audio
                // backend failed. Do NOT emit ptt-down, do NOT change tray icon.
                if crate::settings::pause_media_on_dictate() {
                    crate::media_control::resume();
                }
                tracing::warn!("[hotkey] start ignored: {}", e);
                emit_critical(&app, "dictation-busy", rec.state().to_string());
                session_metrics::record_audio_error();
                return;
            }
            LAST_RECORDING_START_MS.store(now_ms(), Ordering::Release);
            // Recording was accepted. Check if key-up already arrived while
            // this thread was waiting to be scheduled (quick-tap race in hold
            // mode). If so, cancel immediately — don't show the overlay.
            if CANCEL_PENDING.swap(false, Ordering::AcqRel) {
                crate::diagnostic_log::emergency_trace("[ptt_down] cancel_pending after rec.start");
                if crate::settings::pause_media_on_dictate() {
                    crate::media_control::resume();
                }
                rec.cancel();
                tray::set_tray_icon(&tray, TrayState::Idle);
                emit_critical(&app, "recording-cancelled", ());
                session_metrics::record_dictation_discarded();
                return;
            }

            session_metrics::record_dictation_started();

            // Start the segment transcriber now so concurrent mid-recording
            // segment transcriptions can run while the user is still speaking.
            // The receiver was installed in AudioCapture during start(); taking
            // it here transfers ownership to SegmentTranscriber.
            if let Some(seg_rx) = rec.take_segment_receiver() {
                *CURRENT_SEG_TRANSCRIBER.lock() = Some(
                    crate::transcribe::SegmentTranscriber::start(seg_rx, Some(app.clone())),
                );
            }

            // Allocate this job's id.
            let job_id = next_job_id();
            *CURRENT_JOB_ID.lock() = Some(job_id);

            // --- Audio-live gate --------------------------------------------
            // `rec.start()` has returned, but that only means `stream.play()`
            // succeeded — on a cold start CoreAudio doesn't deliver the first
            // input callback for ~200 ms (more with a Bluetooth / route
            // switch). Emitting `ptt-down` now would flash the red "recording"
            // indicator while the mic is still silent: the user starts talking
            // and the leading words are dropped (the pre-roll ring is empty on
            // a cold start, so it can't backfill them). Instead, hold the
            // overlay on the yellow "connecting" tile until the first real
            // audio buffer lands, then flash red. On a warm stream callbacks
            // are already flowing, so `audio_live()` is true within one tick
            // and no yellow flash shows.
            if !rec.audio_live() {
                if !overlay_armed {
                    crate::windowing::reposition_overlay_to_cursor_monitor(&app);
                    emit_critical(&app, "ptt-armed", ());
                }
                crate::diagnostic_log::emergency_trace("[ptt_down] waiting audio_live");
                // Poll up to 2 s for the first callback. 5 ms tick keeps the
                // red flash within a frame of true-live. Bail on:
                //   - CANCEL_PENDING (key released during the wait → cancel),
                //   - the recorder leaving Recording (the level-broadcast
                //     thread cancelled it on a device-lost edge; it already
                //     emitted `device-lost`, so we just exit quietly).
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
                loop {
                    if rec.audio_live() {
                        break;
                    }
                    if CANCEL_PENDING.swap(false, Ordering::AcqRel) {
                        crate::diagnostic_log::emergency_trace(
                            "[ptt_down] cancel_pending during audio_live",
                        );
                        if crate::settings::pause_media_on_dictate() {
                            crate::media_control::resume();
                        }
                        rec.cancel();
                        tray::set_tray_icon(&tray, TrayState::Idle);
                        emit_critical(&app, "recording-cancelled", ());
                        return;
                    }
                    if !rec.is_recording() {
                        crate::diagnostic_log::emergency_trace(format!(
                            "[ptt_down] recorder left recording during audio_live state={}",
                            rec.state()
                        ));
                        tracing::warn!(
                            "[hotkey job_id={}] recorder left Recording during audio-live \
                             wait — aborting ptt-down (device-lost handled elsewhere)",
                            job_id
                        );
                        if crate::settings::pause_media_on_dictate() {
                            crate::media_control::resume();
                        }
                        return;
                    }
                    if std::time::Instant::now() >= deadline {
                        crate::diagnostic_log::emergency_trace(
                            "[ptt_down] audio_live timeout; proceeding",
                        );
                        tracing::warn!(
                            "[hotkey job_id={}] audio-live gate timed out after 2000 ms — \
                             flashing red anyway",
                            job_id
                        );
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }

            // Emit the ptt-down events and tray icon *before* the frontmost-app
            // query so the recording indicator lands immediately.
            // `frontmost_app()` spawns osascript (~50-200ms);
            // audio capture is already running, so moving it after is a
            // pure latency win with no data loss.
            tray::set_tray_icon(&tray, TrayState::Recording);
            // Position before ptt-down so the red recording overlay renders on
            // the monitor currently under the mouse pointer.
            crate::windowing::reposition_overlay_to_cursor_monitor(&app);
            emit_critical(&app, "ptt-down", ());
            emit_stage(&app, job_id, "recording");
            crate::diagnostic_log::emergency_trace(format!("[ptt_down] recording job_id={job_id}"));

            // Capture the frontmost app at recording start (best-effort; may be
            // `None` if the query fails). Stored under its own mutex so the
            // upstroke worker can recover it without entangling JOB_ID's lock.
            // Logged with the job_id so a single grep gives the full lifecycle.
            let focus_start = crate::paste::frontmost_app();
            *FOCUS_AT_START.lock() = Some(focus_start.clone());
            tracing::info!(
                "[hotkey job_id={}] recording started focus_at_start={:?}",
                job_id,
                focus_start
            );
        });
    }

    /// Finish the current job lifecycle and reset tray + stage to Ready.
    /// Used by all completion paths to avoid duplicating the
    /// `finish_guarded` + tray-set_icon + `emit_stage("ready")` sequence.
    /// `call_finish_guarded` is false for the segment-recovery path, which
    /// never entered the Cleaning/Pasting state machine.
    fn bail_out(
        rec: &Recorder,
        tray: &TrayIcon,
        app: &AppHandle,
        job_id_opt: Option<u64>,
        call_finish_guarded: bool,
    ) {
        // Restore playback while the recorder is still busy. Otherwise a new
        // press can start between the Ready transition and the resume toggle.
        if crate::settings::pause_media_on_dictate() {
            crate::media_control::resume();
        }
        if call_finish_guarded {
            rec.finish_guarded();
        }
        tray::set_tray_icon(&tray, TrayState::Idle);
        if let Some(job_id) = job_id_opt {
            emit_stage(app, job_id, "ready");
        }
    }

    /// Paste the final transcript text into the focused app and tear down
    /// the lifecycle. Shared by the normal and salvaged completion paths.
    /// Callers must gate on empty-text and post-cleanup cancel windows
    /// before calling.
    fn paste_and_teardown(
        rec: &Recorder,
        tray: &TrayIcon,
        app: &AppHandle,
        final_text: &str,
        cancel_epoch_at_stop: u64,
        job_id_opt: Option<u64>,
        focus_at_start: &Option<String>,
    ) {
        emit_critical(app, "transcript", final_text.to_string());

        // Cleaning → Pasting
        if rec.begin_pasting().is_err() {
            bail_out(rec, tray, app, job_id_opt, true);
            return;
        }
        if let Some(job_id) = job_id_opt {
            emit_stage(app, job_id, "pasting");
        }
        if job_cancelled_since(cancel_epoch_at_stop) {
            tracing::info!(
                "[hotkey job_id={:?}] cancel observed before paste — suppressing paste",
                job_id_opt
            );
            bail_out(rec, tray, app, job_id_opt, true);
            return;
        }

        // Focus policy: paste into whatever app is frontmost *now*.
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
                    app,
                    "focus-changed-before-paste",
                    FocusChangedBeforePaste {
                        job_id,
                        focus_at_start: Some(start.clone()),
                        focus_at_paste: Some(now.clone()),
                    },
                );
            }
        }
        // Defense-in-depth: verify the recorder is still in Pasting state.
        if !matches!(rec.state(), crate::recorder::State::Pasting) {
            tracing::info!(
                "[hotkey job_id={:?}] paste suppressed — recorder is {:?}, \
                 expected Pasting",
                job_id_opt,
                rec.state()
            );
            bail_out(rec, tray, app, job_id_opt, true);
            return;
        }
        let paste_text = format!("{} ", final_text);
        crate::diagnostic_log::emergency_trace(format!(
            "[paste] start job_id={job_id_opt:?} chars={}",
            final_text.chars().count()
        ));
        match crate::paste::paste(&paste_text, app) {
            Ok(true) => {
                crate::diagnostic_log::emergency_trace(format!(
                    "[paste] ok job_id={job_id_opt:?}"
                ));
                session_metrics::record_paste_success();
                session_metrics::record_dictation_completed();
                play_chime(app, ChimeEvent::Finish);
            }
            Ok(false) => {
                crate::diagnostic_log::emergency_trace(format!(
                    "[paste] copied-fallback job_id={job_id_opt:?}"
                ));
                session_metrics::record_paste_failure();
                emit_critical(
                    app,
                    "paste-copied",
                    "Auto-paste blocked. Copied to clipboard; press Command-V.".to_string(),
                );
            }
            Err(e) => {
                crate::diagnostic_log::emergency_trace(format!(
                    "[paste] error job_id={job_id_opt:?} err={e}"
                ));
                tracing::error!("[paste job_id={:?}] {:?}", job_id_opt, e);
                session_metrics::record_paste_failure();
                emit_critical(
                    app,
                    "paste-error",
                    "Couldn't paste — check Accessibility permission".to_string(),
                );
            }
        }

        // End of lifecycle.
        bail_out(rec, tray, app, job_id_opt, true);
    }

    pub(super) fn ptt_up(recorder: &Arc<Recorder>, tray_icon: &TrayIcon, app: &AppHandle) {
        let rec = recorder.clone();
        let tray = tray_icon.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            boost_thread();
            crate::diagnostic_log::emergency_trace(format!(
                "[ptt_up] enter recorder={} suppress_pending={}",
                rec.state(),
                SUPPRESS_PTT_UP_COUNT.load(Ordering::Acquire),
            ));
            // Cancel-cascade suppression: any cancel path or tap-mash listener
            // that suppressed a record-key down dispatch armed one slot in
            // `SUPPRESS_PTT_UP_COUNT`. Each such pairing's key-up arrives here
            // with the recorder already returned to Ready (or never started).
            // Drop on the floor so we don't fall into IllegalTransition and
            // arm CANCEL_PENDING for the next press.
            if try_consume_ptt_up_suppression() {
                crate::diagnostic_log::emergency_trace("[ptt_up] suppressed");
                return;
            }

            let mode = app.state::<crate::HotkeyState>().read().mode.clone();
            if mode == "toggle" && matches!(rec.state(), crate::recorder::State::Recording) {
                let elapsed_ms =
                    now_ms().saturating_sub(LAST_RECORDING_START_MS.load(Ordering::Acquire));
                if elapsed_ms < 300 {
                    crate::diagnostic_log::emergency_trace(format!(
                        "[ptt_up] ignored toggle debounce elapsed_ms={elapsed_ms}"
                    ));
                    tracing::warn!(
                        "[hotkey] ignored toggle stop {} ms after start (duplicate event debounce)",
                        elapsed_ms
                    );
                    return;
                }
            }

            // Tray-state policy: Recording icon only during literal capture; the
            // moment we enter FinalizingAudio (inside `rec.stop()`) the tray flips
            // to Transcribing and stays that way through Cleaning + Pasting.
            // Idle is restored exactly once at the end of the lifecycle.
            //
            // Capture the cancel epoch BEFORE rec.stop(). The stop/take calls
            // below may overlap with Escape cancel, which increments
            // CANCEL_EPOCH. Capturing early ensures the pre-stop epoch is the
            // one checked throughout the rest of this handler.
            let cancel_epoch_at_stop = cancel_epoch();
            match rec.stop() {
                Ok(StopOutcome::Wav {
                    path,
                    speech_detected,
                    full_capture,
                }) => {
                    // Capture has fully stopped. Restore media now rather
                    // than holding it paused through transcription and paste.
                    if crate::settings::pause_media_on_dictate() {
                        crate::media_control::resume();
                    }
                    // Only now, after rec.stop() succeeded, take the statics
                    // that ptt_down wrote. The Err arm's loser takes nothing.
                    // Recover the job id allocated when this recording started. If the
                    // upstroke arrives without a matching downstroke (no in-flight job),
                    // we still call `rec.stop()` defensively but skip stage emissions.
                    let job_id_opt = CURRENT_JOB_ID.lock().take();
                    crate::diagnostic_log::emergency_trace(format!(
                        "[ptt_up] stop=Wav job_id={job_id_opt:?} speech_detected={speech_detected} full_capture={full_capture}"
                    ));
                    // Recover the focus identity captured at recording start. Outer
                    // `Option` = "was a recording in flight"; inner `Option<String>` =
                    // "did the macOS query succeed". We only compare-and-emit later if
                    // we actually have a job and reach the paste stage.
                    let focus_at_start: Option<String> = FOCUS_AT_START.lock().take().flatten();
                    // Recover the segment transcriber started at key-down. Used in the
                    // Wav arm to assemble concurrent segment results with the tail.
                    // Dropped automatically (joining its worker) in Discard / Err arms.
                    let seg_transcriber_opt = CURRENT_SEG_TRANSCRIBER.lock().take();

                    // We are now in `FinalizingAudio` per recorder contract.
                    tray::set_tray_icon(&tray, TrayState::Transcribing);
                    emit_critical(&app, "ptt-up", ());
                    if let Some(job_id) = job_id_opt {
                        emit_stage(&app, job_id, "finalizing_audio");
                    }

                    // `path` is a tempfile::TempPath — its Drop deletes the WAV
                    // automatically whether we exit via the success arm, the
                    // error arm, or a panic. No explicit cleanup needed.

                    // FinalizingAudio → Transcribing
                    if let Err(e) = rec.begin_transcribing() {
                        crate::diagnostic_log::emergency_trace(format!(
                            "[ptt_up] begin_transcribing failed job_id={job_id_opt:?} err={e}"
                        ));
                        tracing::error!(
                            "[hotkey job_id={:?}] begin_transcribing failed: {}",
                            job_id_opt,
                            e
                        );
                        if crate::settings::pause_media_on_dictate() {
                            crate::media_control::resume();
                        }
                        rec.finish_guarded();
                        tray::set_tray_icon(&tray, TrayState::Idle);
                        session_metrics::record_dictation_discarded();
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

                    // Stage 1: transcribe the WAV. In full-capture mode
                    // (Parakeet) this is the ENTIRE recording in one pass —
                    // whole-utterance context yields coherent punctuation and
                    // capitalization. Otherwise it's the tail (audio after the
                    // last segment cut, or the whole recording if no segments
                    // were emitted — identical to the batch path behavior).
                    // Always transcribe when we have a WAV. The streaming
                    // finalizer already trimmed silence and enforced MIN_RECORDING_MS;
                    // gating on `speech_detected` caused VAD false-negatives to drop
                    // real speech (5+ s of audio written with speech_detected=false).
                    // Hallucinations on true silence are caught by detect_garbage().
                    if !speech_detected {
                        tracing::info!(
                            "[hotkey job_id={:?}] tail speech_detected=false — transcribing anyway",
                            job_id_opt
                        );
                    }
                    let tail_result = crate::transcribe::run_raw(&path);

                    // Stage 2: wait for any in-flight concurrent segment
                    // transcriptions (started at key-down) to finish, then
                    // assemble them in emission order. Segments precede the
                    // tail chronologically. In full-capture mode the WAV
                    // already contains the segments' audio, so their text is
                    // preview-only — join the worker to reap it, discard the
                    // text.
                    let seg_text_all = seg_transcriber_opt
                        .map(|st| st.join_segments())
                        .unwrap_or_default();
                    let seg_text = if full_capture {
                        if !seg_text_all.is_empty() {
                            tracing::info!(
                                "[hotkey job_id={:?}] full-capture: dropping {} chars of \
                                 preview-only segment text",
                                job_id_opt,
                                seg_text_all.chars().count()
                            );
                        }
                        String::new()
                    } else {
                        seg_text_all.clone()
                    };

                    // Run garbage detection on the fully assembled
                    // transcript (segments + tail), not just the tail outcome.
                    // Also carry individual part texts so per-part
                    // garbage checks can salvage clean portions instead of
                    // blocking the entire paste when one part is clean.
                    type TranscribeResult = (
                        String,
                        Option<crate::transcribe::RejectReason>,
                        String,
                        String,
                    );
                    //                                   ^assembled   ^rejection                 ^seg_part   ^tail_part
                    let transcribe_result: anyhow::Result<TranscribeResult> = match tail_result {
                        Ok(outcome) => {
                            let tail_text = outcome.text;
                            let parts: Vec<&str> = [seg_text.as_str(), tail_text.as_str()]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .collect();
                            let assembled = crate::transcribe::strip_trailing_filler(&parts.join(" "));
                            let rejection = if assembled.is_empty() {
                                None
                            } else {
                                crate::transcribe::detect_garbage(&assembled)
                            };
                            Ok((assembled, rejection, seg_text, tail_text))
                        }
                        Err(e) => {
                            // Salvage: even in full-capture mode, if the
                            // one-pass transcription fails outright, the
                            // preview segment text is better than losing the
                            // dictation entirely.
                            if !seg_text_all.is_empty() {
                                tracing::warn!(
                                    "[transcribe job_id={:?}] tail failed, \
                                     using {} chars from segments: {}",
                                    job_id_opt,
                                    seg_text_all.chars().count(),
                                    e
                                );
                                Ok((seg_text_all, None, String::new(), String::new()))
                            } else {
                                session_metrics::record_transcribe_error();
                                Err(e)
                            }
                        }
                    };

                    // Transcribing → Cleaning (always — even on whisper error,
                    // so the lifecycle reaches `finish` through legal transitions).
                    if rec.begin_cleaning().is_err() {
                        // Recorder forced out from under us (e.g. cancel).
                        // A new job may have already started (Recording state).
                        if crate::settings::pause_media_on_dictate() {
                            crate::media_control::resume();
                        }
                        rec.finish_guarded();
                        tray::set_tray_icon(&tray, TrayState::Idle);
                        session_metrics::record_dictation_discarded();
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
                        Ok((raw_text, rejection, seg_part, tail_part)) => {
                            tracing::info!(
                                "[transcribe job_id={:?}] raw transcript received ({} chars)",
                                job_id_opt,
                                raw_text.chars().count()
                            );

                            // Garbage detection is advisory — paste the full text
                            // regardless, with an appropriate UI flag. Never
                            // truncate: partially clean text is still more useful
                            // than nor pasting anything.
                            if let Some(ref reason) = rejection {
                                // Check if either individual part is clean on its
                                // own — if so this is a partial rejection (mild
                                // flag) rather than a full flaky rejection.
                                let seg_garbage = if seg_part.is_empty() {
                                    false
                                } else {
                                    crate::transcribe::detect_garbage(&seg_part).is_some()
                                };
                                let tail_garbage = if tail_part.is_empty() {
                                    false
                                } else {
                                    crate::transcribe::detect_garbage(&tail_part).is_some()
                                };
                                let is_partial = !seg_garbage || !tail_garbage;

                                let reason_str = if is_partial {
                                    format!("partial_rejection — {:?}", reason)
                                } else {
                                    reason.description().to_string()
                                };

                                tracing::warn!(
                                    "[cleanup job_id={:?}] transcript rejected ({:?}) — \
                                     pasting full text with {} flag",
                                    job_id_opt,
                                    reason,
                                    if is_partial { "partial" } else { "flaky" },
                                );
                                emit_critical(
                                    &app,
                                    "transcription-rejected",
                                    serde_json::json!({
                                        "text": raw_text,
                                        "reason": reason_str,
                                        "label": reason.label(),
                                        "pasted": true,
                                        "flaky": !is_partial,
                                    }),
                                );
                                play_chime(&app, ChimeEvent::Error);
                                // Overlay shows the yellow toast — no need to
                                // open the main window and steal focus.
                            }

                            // Stage 2: cleanup as its own explicit call site.
                            let final_text = crate::cleanup::process(&raw_text);
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
                                session_metrics::record_dictation_discarded();
                                emit_critical(&app, "recording-discarded", "empty-final-text");
                                bail_out(&rec, &tray, &app, job_id_opt, true);
                                play_chime(&app, ChimeEvent::Finish);
                                return;
                            }
                            if wait_for_hold_cancel_window(cancel_epoch_at_stop, job_id_opt) {
                                tracing::info!(
                                    "[hotkey job_id={:?}] cancel observed after cleanup/hold window — suppressing transcript/paste",
                                    job_id_opt
                                );
                                bail_out(&rec, &tray, &app, job_id_opt, true);
                                return;
                            }
                            paste_and_teardown(
                                &rec,
                                &tray,
                                &app,
                                &final_text,
                                cancel_epoch_at_stop,
                                job_id_opt,
                                &focus_at_start,
                            );
                        }
                        Err(e) => {
                            crate::diagnostic_log::emergency_trace(format!(
                                "[ptt_up] transcript-error job_id={job_id_opt:?} err={e}"
                            ));
                            tracing::error!("[transcribe job_id={:?}] {:?}", job_id_opt, e);
                            let msg = format!("{}", e);
                            emit_critical(&app, "transcript-error", msg);
                        }
                    }

                    // End of lifecycle — back to Ready regardless of which arm we
                    // took (success, transcribe error, or paste error).
                    if crate::settings::pause_media_on_dictate() {
                        crate::media_control::resume();
                    }
                    rec.finish_guarded();
                    tray::set_tray_icon(&tray, TrayState::Idle);
                    if let Some(job_id) = job_id_opt {
                        emit_stage(&app, job_id, "ready");
                    }
                    // `path` drops here → WAV file deleted from /tmp.
                }
                Ok(StopOutcome::Discard(reason)) => {
                    if crate::settings::pause_media_on_dictate() {
                        crate::media_control::resume();
                    }
                    // Take statics only after rec.stop() succeeded, matching
                    // the Wav arm's post-stop read pattern.
                    let job_id_opt = CURRENT_JOB_ID.lock().take();
                    crate::diagnostic_log::emergency_trace(format!(
                        "[ptt_up] stop=Discard job_id={job_id_opt:?} reason={reason:?}"
                    ));
                    let focus_at_start: Option<String> = FOCUS_AT_START.lock().take().flatten();
                    let seg_transcriber_opt = CURRENT_SEG_TRANSCRIBER.lock().take();

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
                            tray::set_tray_icon(&tray, TrayState::Transcribing);
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
                                let final_text = crate::cleanup::process(&seg_text);

                                // Re-check: a new job may have started during process.
                                // window, the old recovery must not emit,
                                // paste, or touch the tray — the new job owns
                                // the UI state now.
                                if CURRENT_JOB_ID.lock().is_some() {
                                    tracing::warn!(
                                        "[hotkey job_id={:?}] seg-recovery: new job started during \
                                         cleanup::process — abandoning recovery to protect new job's UI",
                                        job_id_opt
                                    );
                                    return;
                                }

                                if final_text.is_empty() {
                                    emit_critical(&app, "recording-discarded", "empty-final-text");
                                    play_chime(&app, ChimeEvent::Finish);
                                } else {
                                    if wait_for_hold_cancel_window(cancel_epoch_at_stop, job_id_opt)
                                    {
                                        tracing::info!(
                                            "[hotkey job_id={:?}] seg-recovery: cancel observed after cleanup/hold window — suppressing paste",
                                            job_id_opt
                                        );
                                        bail_out(&rec, &tray, &app, job_id_opt, false);
                                        return;
                                    }
                                    // Segment recovery fires only when the tail
                                    // after the last silence-boundary cut is under
                                    // MIN_RECORDING_MS — i.e. trailing sub-threshold
                                    // silence. All real speech is already in the
                                    // joined segments, so `final_text` is the complete
                                    // dictation, not a partial chunk. Carry it in the
                                    // payload so the frontend can record a history
                                    // entry (same as `transcript`). A distinct event
                                    // name is kept so the overlay can still differentiate
                                    // recovery from a normal finish.
                                    emit_critical(&app, "recording-recovered", final_text.clone());
                                    if let Some(job_id) = job_id_opt {
                                        emit_stage(&app, job_id, "pasting");
                                    }
                                    if job_cancelled_since(cancel_epoch_at_stop) {
                                        tracing::info!(
                                            "[hotkey job_id={:?}] seg-recovery: cancel observed before paste — suppressing paste",
                                            job_id_opt
                                        );
                                        bail_out(&rec, &tray, &app, job_id_opt, false);
                                        return;
                                    }
                                    let focus_at_paste = crate::paste::frontmost_app();
                                    tracing::info!(
                                        "[paste job_id={:?}] (seg-recovery) focus_at_start={:?} focus_at_paste={:?}",
                                        job_id_opt, focus_at_start, focus_at_paste
                                    );
                                    if let (Some(job_id), Some(start), Some(now)) = (
                                        job_id_opt,
                                        focus_at_start.as_ref(),
                                        focus_at_paste.as_ref(),
                                    ) {
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
                                    crate::diagnostic_log::emergency_trace(format!(
                                        "[paste] seg-recovery start job_id={job_id_opt:?} chars={}",
                                        final_text.chars().count()
                                    ));
                                    match crate::paste::paste(&paste_text, &app) {
                                        Ok(true) => {
                                            crate::diagnostic_log::emergency_trace(format!(
                                                "[paste] seg-recovery ok job_id={job_id_opt:?}"
                                            ));
                                            session_metrics::record_paste_success();
                                            session_metrics::record_dictation_completed();
                                 play_chime(&app, ChimeEvent::Finish);
                                        }
                                        Ok(false) => {
                                            crate::diagnostic_log::emergency_trace(format!(
                                                "[paste] seg-recovery copied-fallback job_id={job_id_opt:?}"
                                            ));
                                            session_metrics::record_paste_failure();
                                            emit_critical(
                                                &app,
                                                "paste-copied",
                                                "Auto-paste blocked. Copied to clipboard; press Command-V."
                                                    .to_string(),
                                            );
                                        }
                                        Err(e) => {
                                            crate::diagnostic_log::emergency_trace(format!(
                                                "[paste] seg-recovery error job_id={job_id_opt:?} err={e}"
                                            ));
                                            tracing::error!(
                                                "[paste job_id={:?}] (seg-recovery) {:?}",
                                                job_id_opt,
                                                e
                                            );
                                            session_metrics::record_paste_failure();
                                            emit_critical(
                                                &app,
                                                "paste-error",
                                                "Couldn't paste — check Accessibility permission"
                                                    .to_string(),
                                            );
                                        }
                                    }
                                }
                            } else {
                                // Segments produced no text — normal discard.
                                session_metrics::record_dictation_discarded();
                                emit_critical(&app, "recording-discarded", ());
                            }
                            bail_out(&rec, &tray, &app, job_id_opt, false);
                            return;
                        }
                    }
                    // Normal discard path — `recording-discarded` is the catch-all
                    // the overlay listens to; `recording-too-short` is the more
                    // specific subtype the main window uses to show a toast.
                    if crate::settings::pause_media_on_dictate() {
                        crate::media_control::resume();
                    }
                    tray::set_tray_icon(&tray, TrayState::Idle);
                    session_metrics::record_dictation_discarded();
                    if let DiscardReason::TooShort { duration_ms } = reason {
                        crate::diagnostic_log::emergency_trace(format!(
                            "[ptt_up] recording-too-short job_id={job_id_opt:?} duration_ms={duration_ms}"
                        ));
                        emit_critical(&app, "recording-too-short", duration_ms);
                    }
                    emit_critical(&app, "recording-discarded", ());
                    if let Some(job_id) = job_id_opt {
                        emit_stage(&app, job_id, "ready");
                    }
                }
                Err(e) => {
                    if crate::settings::pause_media_on_dictate() {
                        crate::media_control::resume();
                    }
                    crate::diagnostic_log::emergency_trace(format!(
                        "[ptt_up] stop error state={} err={e}",
                        rec.state()
                    ));
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
                        if START_IN_FLIGHT.load(Ordering::Acquire)
                            || crate::transcribe::prewarm_in_flight()
                        {
                            CANCEL_PENDING.store(true, Ordering::Release);
                            crate::diagnostic_log::emergency_trace(
                                "[ptt_up] set cancel_pending for quick-tap race",
                            );
                        } else {
                            crate::diagnostic_log::emergency_trace(
                                "[ptt_up] ignored idle key-up without pending start",
                            );
                            tracing::debug!("idle key-up ignored without pending start");
                        }
                    }
                    if crate::settings::pause_media_on_dictate() {
                        crate::media_control::resume();
                    }
                    tracing::warn!("stop ignored: {}", e);
                    tray::set_tray_icon(&tray, TrayState::Idle);
                }
            }
        });
    }
}

// ── Controller layer ───────────────────────────────────────────────────
//
// HoldController and ToggleController each own their mode-specific
// lifecycle rules. Both call into the shared engine (`mod common`) for
// the actual record / transcribe / cleanup / paste work.

/// Unified dispatch enum — constructed once per event by platform code.
pub(super) enum Controller {
    Hold(HoldController),
    Toggle(ToggleController),
    Auto(AutoController),
}

impl Controller {
    pub fn from_mode(
        mode: &str,
        recorder: &std::sync::Arc<crate::recorder::Recorder>,
        tray_icon: &tauri::tray::TrayIcon,
        app: &tauri::AppHandle,
    ) -> Self {
        match mode {
            "toggle" => Self::Toggle(ToggleController::new(recorder, tray_icon, app)),
            "auto" => Self::Auto(AutoController::new(recorder, tray_icon, app)),
            _ => Self::Hold(HoldController::new(recorder, tray_icon, app)),
        }
    }

    pub fn press(&self) {
        match self {
            Self::Hold(c) => c.press(),
            Self::Toggle(c) => c.press(),
            Self::Auto(c) => c.press(),
        }
    }

    pub fn release(&self) {
        match self {
            Self::Hold(c) => c.release(),
            Self::Toggle(c) => c.release(),
            Self::Auto(c) => c.release(),
        }
    }

    /// Arm hold-to-cancel if the recorder is busy. Does nothing otherwise.
    /// HoldController also arms ptt_up suppression; ToggleController does not.
    pub fn arm_hold_cancel_if_busy(&self) {
        match self {
            Self::Hold(c) => c.arm_hold_cancel_if_busy(),
            Self::Toggle(c) => c.arm_hold_cancel_if_busy(),
            Self::Auto(c) => c.arm_hold_cancel_if_busy(),
        }
    }

    /// Cancel any in-flight recording (Escape, tray click, IPC).
    pub fn cancel_if_busy(&self) {
        match self {
            Self::Hold(c) => c.cancel_if_busy(),
            Self::Toggle(c) => c.cancel_if_busy(),
            Self::Auto(c) => c.cancel_if_busy(),
        }
    }
}

/// Hold-mode controller — press starts recording, release stops.
/// Simple press / release semantics with hold-to-cancel support.
pub(super) struct HoldController {
    recorder: std::sync::Arc<crate::recorder::Recorder>,
    tray_icon: tauri::tray::TrayIcon,
    app: tauri::AppHandle,
}

impl HoldController {
    fn new(
        recorder: &std::sync::Arc<crate::recorder::Recorder>,
        tray_icon: &tauri::tray::TrayIcon,
        app: &tauri::AppHandle,
    ) -> Self {
        Self {
            recorder: recorder.clone(),
            tray_icon: tray_icon.clone(),
            app: app.clone(),
        }
    }

    /// Press the PTT key — always starts recording (or queues a start).
    pub fn press(&self) {
        common::ptt_down(&self.recorder, &self.tray_icon, &self.app);
    }

    /// Release the PTT key — always stops and processes transcription.
    pub fn release(&self) {
        common::disarm_hold_cancel();
        common::ptt_up(&self.recorder, &self.tray_icon, &self.app);
    }

    /// Arm hold-to-cancel if recorder is busy. Arms ptt_up suppression
    /// because key-release is always on its way in hold mode.
    pub fn arm_hold_cancel_if_busy(&self) {
        if common::should_arm_hold_cancel(&self.recorder) {
            common::arm_ptt_up_suppression();
            common::arm_hold_cancel(&self.recorder, &self.tray_icon, &self.app);
        }
    }

    /// Cancel any in-flight recording. Arms ptt_up suppression.
    pub fn cancel_if_busy(&self) {
        if self.recorder.state().is_busy() {
            common::arm_ptt_up_suppression();
            common::trigger_cancel(&self.recorder, &self.tray_icon, &self.app);
        }
    }
}

/// Toggle-mode controller — press toggles recording on / off.
/// Has its own arming-cancel and debounce logic; release is a no-op.
pub(super) struct ToggleController {
    recorder: std::sync::Arc<crate::recorder::Recorder>,
    tray_icon: tauri::tray::TrayIcon,
    app: tauri::AppHandle,
}

impl ToggleController {
    fn new(
        recorder: &std::sync::Arc<crate::recorder::Recorder>,
        tray_icon: &tauri::tray::TrayIcon,
        app: &tauri::AppHandle,
    ) -> Self {
        Self {
            recorder: recorder.clone(),
            tray_icon: tray_icon.clone(),
            app: app.clone(),
        }
    }

    /// Press the PTT key — toggles recording on / off.
    /// Includes 300 ms debounce and arming-cancel for warmup.
    pub fn press(&self) {
        if self.recorder.is_recording() {
            // Debounce: ignore stop within 300 ms of start. CGEventTap + IOHID
            // can both fire for the same physical press — without this guard the
            // second event would immediately stop the recording.
            let elapsed_ms = common::now_ms()
                .saturating_sub(common::LAST_RECORDING_START_MS.load(std::sync::atomic::Ordering::Acquire));
            if elapsed_ms < 300 {
                crate::diagnostic_log::emergency_trace(format!(
                    "[ptt_up] ignored toggle debounce elapsed_ms={elapsed_ms}"
                ));
                tracing::warn!(
                    "[hotkey] ignored toggle stop {} ms after start (duplicate event debounce)",
                    elapsed_ms
                );
                return;
            }
            common::ptt_up(&self.recorder, &self.tray_icon, &self.app);
        } else {
            // Cancel arming: if whisper-server is still loading from a prior
            // press, treat this as cancel. There is no key-up in toggle mode so
            // CANCEL_PENDING is never set — CANCEL_ARMING signals the poll loop.
            if crate::transcribe::prewarm_in_flight() {
                crate::diagnostic_log::emergency_trace(
                    "[ptt_down] cancel arming prewarm_in_flight",
                );
                tracing::info!(
                    "[hotkey] arm cancelled — user pressed again during warmup (toggle mode)"
                );
                common::CANCEL_ARMING.store(true, std::sync::atomic::Ordering::Release);
                return;
            }
            common::ptt_down(&self.recorder, &self.tray_icon, &self.app);
        }
    }

    /// Release is a no-op in toggle mode.
    pub fn release(&self) {
        common::disarm_hold_cancel();
    }

    /// Arm hold-to-cancel if recorder is busy. Does NOT arm ptt_up
    /// suppression because key-release is a no-op in toggle mode.
    pub fn arm_hold_cancel_if_busy(&self) {
        if common::should_arm_hold_cancel(&self.recorder) {
            common::arm_hold_cancel(&self.recorder, &self.tray_icon, &self.app);
        }
    }

    /// Cancel any in-flight recording. Does NOT arm ptt_up suppression.
    pub fn cancel_if_busy(&self) {
        if self.recorder.state().is_busy() {
            common::trigger_cancel(&self.recorder, &self.tray_icon, &self.app);
        }
    }
}

/// Auto-mode controller — hybrid: quick tap toggles, long hold acts as PTT.
///
/// Delegates to the existing Hold and Toggle controllers based on press
/// duration:
///
/// | Action | State | Delegate | Effect |
/// |---|---|---|---|
/// | Press | Idle | `Hold::press` | Start recording |
/// | Release < threshold | Idle→Recording | `Toggle::release` | No-op — continue hands-free |
/// | Release ≥ threshold | Idle→Recording | `Hold::release` | Stop (PTT) |
/// | Press | Recording | `Toggle::press` | Stop (toggle) |
/// | Release | Any | `Hold::release` if held; else no-op | Stop or continue |
///
/// The threshold defaults to 400 ms and is configurable via
/// `HotkeyConfig::auto_tap_threshold_ms`.
pub(super) struct AutoController {
    hold: HoldController,
    toggle: ToggleController,
}

/// The controller is rebuilt for each platform input event, so the press time
/// must live outside the short-lived controller to survive key-down → key-up.
static AUTO_PRESS_TIME_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static AUTO_PRESS_WAS_BUSY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl AutoController {
    fn new(
        recorder: &std::sync::Arc<crate::recorder::Recorder>,
        tray_icon: &tauri::tray::TrayIcon,
        app: &tauri::AppHandle,
    ) -> Self {
        Self {
            hold: HoldController::new(recorder, tray_icon, app),
            toggle: ToggleController::new(recorder, tray_icon, app),
        }
    }

    pub fn press(&self) {
        let was_busy = self.hold.recorder.state().is_busy();
        AUTO_PRESS_WAS_BUSY.store(was_busy, std::sync::atomic::Ordering::Release);
        let now = common::now_ms();
        AUTO_PRESS_TIME_MS.store(now, std::sync::atomic::Ordering::Release);

        if was_busy {
            // A second press needs to remain held long enough to distinguish a
            // toggle-stop tap from the configured hold-to-cancel gesture.
            tracing::info!("[auto] press while busy -> waiting for tap/cancel decision timestamp_ms={now}");
        } else {
            // Idle — start recording.
            tracing::info!("[auto] press idle timestamp_ms={now}");
            self.hold.press();
        }
    }

    pub fn release(&self) {
        // Reload threshold from settings (may have been changed at runtime).
        let cfg = crate::settings::load();
        let threshold = cfg.hotkey.auto_tap_threshold_ms;
        let elapsed = common::now_ms()
            .saturating_sub(AUTO_PRESS_TIME_MS.load(std::sync::atomic::Ordering::Acquire));
        let was_busy = AUTO_PRESS_WAS_BUSY.swap(false, std::sync::atomic::Ordering::AcqRel);

        if was_busy {
            if elapsed < threshold {
                tracing::info!(
                    "[auto] busy release elapsed_ms={elapsed} threshold_ms={threshold} -> toggle stop"
                );
                common::disarm_hold_cancel();
                self.toggle.press();
            } else if self.hold.recorder.state().is_busy() {
                tracing::info!(
                    "[auto] busy release elapsed_ms={elapsed} threshold_ms={threshold} -> stop/cancel"
                );
                self.hold.release();
            } else {
                // Hold-cancel already moved the recorder out of its busy state.
                common::disarm_hold_cancel();
                tracing::info!("[auto] busy release after hold-cancel -> ignored");
            }
            return;
        }

        if elapsed < threshold {
            // Quick tap — recording continues hands-free (toggle-style).
            tracing::info!(
                "[auto] release elapsed_ms={elapsed} threshold_ms={threshold} -> keep recording"
            );
            self.toggle.release();
        } else {
            // Long hold — stop on release (hold-style).
            tracing::info!(
                "[auto] release elapsed_ms={elapsed} threshold_ms={threshold} -> stop recording"
            );
            self.hold.release();
        }
    }

    /// Arm hold-to-cancel if busy. Auto mode can receive a key-up, so arm
    /// ptt_up suppression like Hold mode does.
    pub fn arm_hold_cancel_if_busy(&self) {
        if common::should_arm_hold_cancel(&self.hold.recorder) {
            common::arm_hold_cancel(&self.hold.recorder, &self.hold.tray_icon, &self.hold.app);
        }
    }

    /// Cancel any in-flight recording. Arms ptt_up suppression like Hold.
    pub fn cancel_if_busy(&self) {
        if self.hold.recorder.state().is_busy() {
            common::arm_ptt_up_suppression();
            common::trigger_cancel(&self.hold.recorder, &self.hold.tray_icon, &self.hold.app);
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::common;
    use super::Controller;
    use crate::recorder::Recorder;
    use core_foundation::base::TCFType;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType, EventField,
    };
    use std::os::raw::c_void;
    use crossbeam_channel::bounded;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
    use std::sync::Arc;

    use tauri::{tray::TrayIcon, AppHandle, Manager};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;

        /// Enable or disable an event tap.
        fn CGEventTapEnable(tap: *const c_void, enable: bool);
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

    /// kVK_F13–kVK_F19 from `Carbon/HIToolbox/Events.h`.
    fn fkey_code_for_name(name: &str) -> Option<i64> {
        match name {
            "f13" => Some(0x69),
            "f14" => Some(0x6B),
            "f15" => Some(0x71),
            "f16" => Some(0x6A),
            "f17" => Some(0x40),
            "f18" => Some(0x4F),
            "f19" => Some(0x50),
            _ => None,
        }
    }

    /// True while the configured F-key is physically held; cleared on KeyUp.
    /// Prevents macOS autorepeat from firing ptt_down on every repeated event.
    static FKEY_DOWN: AtomicBool = AtomicBool::new(false);

    // ── IOHIDManager raw HID mouse button listener ──────────────────────────
    //
    // Opens an IOHIDManager and reads raw HID value callbacks. This catches
    // mouse button events even when Logi Options+ (or similar software) has
    // "disabled" the button at the IOKit level — IOKit delivers HID input
    // reports to ALL registered IOHIDManager clients, so Logi cannot block
    // us from seeing the raw report.
    //
    // HID Button usage IDs (standard USB HID spec):
    //   Usage 1 = left click, 2 = right click, 3 = middle, 4 = back, 5 = forward

    /// HID usage page for buttons.
    const K_HIDPAGE_BUTTON: u32 = 0x09;

    /// Map config name to HID Button usage ID.
    fn hid_mouse_usage_for_name(name: &str) -> Option<u32> {
        match name {
            "mouse_middle" => Some(3),
            "mouse_back" => Some(4),
            "mouse_forward" => Some(5),
            _ => None,
        }
    }

    /// Bitmask tracking which raw HID mouse buttons are currently held.
    /// Bit N = 1 when HID Button usage ID N is in the pressed state.
    /// Only the IOHIDManager callback thread writes this; relaxed ordering
    /// is safe because the thread is serial (single CFRunLoop).
    static HID_BUTTON_STATE: AtomicU32 = AtomicU32::new(0);
    static HID_KEYBOARD_DOWN: AtomicBool = AtomicBool::new(false);

    /// True while the CGEventTap is active and handling keyboard events.
    /// When set, the IOHID keyboard handler skips keyboard-page events to
    /// prevent the dual-fire race: CGEventTap fires slightly after IOHID for
    /// the same physical keypress, sees is_recording=true (IOHID already
    /// started recording), and mistakenly issues ptt_up — stopping the
    /// recording 162µs after it started.
    static CGEVENTTAP_ACTIVE: AtomicBool = AtomicBool::new(false);

    /// Set to true the moment the IOHIDManager listener thread is running.
    /// Used by `permissions.rs` to report Input Monitoring as effectively
    /// granted even when `IOHIDCheckAccess` returns Unknown (TCC can lag
    /// briefly after a binary update while it re-verifies the code signature).
    static IOHID_LISTENER_RUNNING: AtomicBool = AtomicBool::new(false);

    /// Minimal event data captured from the CGEventTap callback and shipped to
    /// the processing thread. Keeping this small ensures the callback returns
    /// near-instantly (no heap allocation beyond the channel send).
    struct TapEvent {
        keycode: i64,
        flags: CGEventFlags,
        etype: CGEventType,
    }

    /// Raw Mach port pointer for the CGEventTap, written once after tap creation
    /// and read in the callback to re-enable a tap that macOS disabled.
    static TAP_MACH_PORT_RAW: AtomicUsize = AtomicUsize::new(0);

    pub fn iohid_listener_running() -> bool {
        IOHID_LISTENER_RUNNING.load(Ordering::Acquire)
    }

    fn hid_usage_bit(usage: u32) -> u32 {
        1u32 << usage
    }

    // Raw pointer aliases for CoreFoundation / IOKit opaque types used in
    // the extern "C" declarations below. We keep them module-scoped so the
    // FFI signatures are self-contained and don't depend on core-foundation
    // crate type interop.
    type CFAllocatorRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFRunLoopRef = *const c_void;
    type IOHIDManagerRef = *mut c_void;
    type IOHIDValueRef = *mut c_void;
    type IOHIDElementRef = *mut c_void;

    type IOHIDValueCallback = unsafe extern "C" fn(
        context: *mut c_void,
        result: i32,
        sender: *mut c_void,
        value: IOHIDValueRef,
    );

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFAllocatorDefault: CFAllocatorRef;
        static kCFRunLoopDefaultMode: CFStringRef;
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopRun();
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOHIDManagerCreate(allocator: CFAllocatorRef, options: u32) -> IOHIDManagerRef;
        fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: CFDictionaryRef);
        fn IOHIDManagerRegisterInputValueCallback(
            manager: IOHIDManagerRef,
            callback: IOHIDValueCallback,
            context: *mut c_void,
        );
        fn IOHIDManagerScheduleWithRunLoop(
            manager: IOHIDManagerRef,
            runloop: CFRunLoopRef,
            mode: CFStringRef,
        );
        fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: u32) -> i32;
        fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
        fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
        fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
        fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> isize;
    }

    /// Context passed to the IOHIDManager input-value callback via raw pointer.
    /// Heap-allocated and leaked for the process lifetime (same pattern as the
    /// existing CGEventTap — the OS cleans up on exit).
    struct HidMouseCtx {
        recorder: Arc<Recorder>,
        tray_icon: TrayIcon,
        app: AppHandle,
        hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
    }

    /// IOHIDManager input-value callback. Fires for every HID value change on
    /// every matched device. Filters for Button usage page (0x09) for mouse
    /// buttons and Keyboard usage page (0x07) for keyboard hotkeys.
    /// Runs on the IOHIDManager's CFRunLoop thread — serial, so no concurrent
    /// invocations.
    unsafe extern "C" fn hid_mouse_value_callback(
        ctx: *mut c_void,
        _result: i32,
        _sender: *mut c_void,
        value: IOHIDValueRef,
    ) {
        let context = &*(ctx as *const HidMouseCtx);

        let element = IOHIDValueGetElement(value);
        let usage_page = IOHIDElementGetUsagePage(element);

        if usage_page == K_HIDPAGE_BUTTON {
            let usage = IOHIDElementGetUsage(element);
            // Only react to buttons 3, 4, 5 (middle, back, forward)
            if !(3..=5).contains(&usage) {
                return;
            }

            let int_value = IOHIDValueGetIntegerValue(value);
            let pressed = int_value != 0;

            let (controller, cancel_on_hold) = {
                let hk = context.hotkey_state.read();
                let target = hid_mouse_usage_for_name(&hk.key);
                if target != Some(usage) {
                    return;
                }
                (Controller::from_mode(&hk.mode, &context.recorder, &context.tray_icon, &context.app), hk.cancel_on_hold)
            };

            let bit = hid_usage_bit(usage);
            let was_down = HID_BUTTON_STATE.load(Ordering::Relaxed) & bit != 0;

            if pressed {
                if !was_down {
                    HID_BUTTON_STATE.fetch_or(bit, Ordering::Relaxed);
                    if cancel_on_hold {
                        controller.arm_hold_cancel_if_busy();
                    }
                    controller.press();
                }
            } else if was_down {
                HID_BUTTON_STATE.fetch_and(!bit, Ordering::Relaxed);
                controller.release();
            }

            return;
        }

        if usage_page == K_HIDPAGE_KEYBOARD {
            // CGEventTap handles keyboard events when Accessibility is
            // granted. Firing from IOHID at the same time causes a race:
            // IOHID starts recording, then CGEventTap sees is_recording=true
            // and immediately stops it. Skip when CGEventTap is active.
            if CGEVENTTAP_ACTIVE.load(Ordering::Acquire) {
                return;
            }
            let usage = IOHIDElementGetUsage(element);
            let (target_usage, key_name, mode_name) = {
                let hk = context.hotkey_state.read();
                (
                    keyboard_hid_usage_for_name(&hk.key),
                    hk.key.clone(),
                    hk.mode.clone(),
                )
            };
            let Some(target) = target_usage else { return };
            if usage != target {
                return;
            }

            let int_value = IOHIDValueGetIntegerValue(value);
            let pressed = int_value != 0;

            let (controller, cancel_on_hold) = {
                let hk = context.hotkey_state.read();
                (Controller::from_mode(&hk.mode, &context.recorder, &context.tray_icon, &context.app), hk.cancel_on_hold)
            };

            if pressed {
                if HID_KEYBOARD_DOWN.swap(true, Ordering::Relaxed) {
                    crate::diagnostic_log::emergency_trace(format!(
                        "[hid-keyboard] ignored repeat key={key_name} usage={usage}"
                    ));
                    return;
                }
                crate::diagnostic_log::emergency_trace(format!(
                    "[hid-keyboard] down key={key_name} mode={mode_name} usage={usage} recorder={}",
                    context.recorder.state()
                ));
                if cancel_on_hold {
                    controller.arm_hold_cancel_if_busy();
                }
                controller.press();
            } else {
                if !HID_KEYBOARD_DOWN.swap(false, Ordering::Relaxed) {
                    crate::diagnostic_log::emergency_trace(format!(
                        "[hid-keyboard] ignored up-without-down key={key_name} usage={usage}"
                    ));
                    return;
                }
                crate::diagnostic_log::emergency_trace(format!(
                    "[hid-keyboard] up key={key_name} mode={mode_name} usage={usage} recorder={}",
                    context.recorder.state()
                ));
                controller.release();
            }
        }
    }

    /// Spawn a background thread that reads raw HID mouse button events via
    /// IOHIDManager. This bypasses CGEventTap entirely for mouse buttons, so
    /// Logi Options+ (or other mouse-driver software) cannot intercept the
    /// events before we see them.
    fn spawn_hid_mouse_listener(
        recorder: Arc<Recorder>,
        tray_icon: TrayIcon,
        app: AppHandle,
        hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
    ) {
        std::thread::spawn(move || {
            let ctx = Box::into_raw(Box::new(HidMouseCtx {
                recorder,
                tray_icon,
                app,
                hotkey_state,
            }));

            // SAFETY: IOHIDManagerCreate returns an owned +1 retained ref.
            let manager = unsafe { IOHIDManagerCreate(kCFAllocatorDefault, 0) };
            if manager.is_null() {
                tracing::error!("[hotkey] IOHIDManagerCreate failed for HID mouse listener");
                let _ = unsafe { Box::from_raw(ctx) };
                return;
            }

            // NULL matching dict = match all HID devices. We filter in the
            // callback by usage page + usage ID, so there's no need to build
            // a CFDictionary for device matching.
            unsafe {
                IOHIDManagerSetDeviceMatching(manager, std::ptr::null());
            }

            unsafe {
                IOHIDManagerRegisterInputValueCallback(
                    manager,
                    hid_mouse_value_callback,
                    ctx as *mut c_void,
                );
                IOHIDManagerScheduleWithRunLoop(
                    manager,
                    CFRunLoopGetCurrent(),
                    kCFRunLoopDefaultMode,
                );
                IOHIDManagerOpen(manager, 0);
            }

            IOHID_LISTENER_RUNNING.store(true, Ordering::Release);
            tracing::info!("[hotkey] IOHIDManager mouse listener running");

            // Block the thread forever — the IOHIDManager delivers callbacks
            // on this run loop.
            unsafe { CFRunLoopRun() };

            // Unreachable in normal operation; cleanup on process exit.
            let _ = unsafe { Box::from_raw(ctx) };
        });
    }

    /// Watchdog thread that periodically checks if Input Monitoring permission
    /// has been revoked at runtime. When the user unchecks TurboTalk in
    /// System Settings → Privacy & Security → Input Monitoring while the app
    /// is running, the IOHIDManager stops delivering callbacks silently.
    /// This watchdog detects the loss and emits a ui-error toast so the
    /// user knows why the hotkey stopped working.
    fn spawn_im_watchdog(app: AppHandle) {
        const K_IOHID_REQUEST_TYPE_LISTEN_EVENT: u32 = 1;
        const K_IOHID_ACCESS_TYPE_GRANTED: u32 = 0;
        const K_IOHID_ACCESS_TYPE_DENIED: u32 = 1;

        #[link(name = "IOKit", kind = "framework")]
        extern "C" {
            fn IOHIDCheckAccess(request_type: u32) -> u32;
        }

        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));

            let status = unsafe { IOHIDCheckAccess(K_IOHID_REQUEST_TYPE_LISTEN_EVENT) };
            let is_running = IOHID_LISTENER_RUNNING.load(Ordering::Acquire);

            if status == K_IOHID_ACCESS_TYPE_DENIED && is_running {
                IOHID_LISTENER_RUNNING.store(false, Ordering::Release);
                crate::emit_ui_error(
                    &app,
                    "user-permission-lost",
                    "Input Monitoring permission was revoked. TurboTalk hotkeys are disabled. Re-enable in System Settings → Privacy & Security → Input Monitoring.",
                    false,
                );
                tracing::warn!("[hotkey] Input Monitoring permission revoked at runtime — IOHIDManager events blocked");
            } else if status == K_IOHID_ACCESS_TYPE_GRANTED && !is_running {
                IOHID_LISTENER_RUNNING.store(true, Ordering::Release);
                tracing::info!("[hotkey] Input Monitoring permission re-granted — IOHIDManager should resume delivering events");
            }
        });
    }

    /// Keyboard/kp usage page from USB HID spec.
    const K_HIDPAGE_KEYBOARD: u32 = 0x07;

    /// Map a TurboTalk hotkey name to its USB HID usage code on page 0x07.
    /// Handles modifier keys (right_option, right_control, etc.) and
    /// function keys (f13–f19).
    fn keyboard_hid_usage_for_name(name: &str) -> Option<u32> {
        match name {
            "right_option" => Some(0xE6),  // Right Alt / Option
            "right_control" => Some(0xE4), // Right Control
            "right_command" => Some(0xE7), // Right GUI / Command
            "right_shift" => Some(0xE5),   // Right Shift
            "f13" => Some(0x68),
            "f14" => Some(0x69),
            "f15" => Some(0x6A),
            "f16" => Some(0x6B),
            "f17" => Some(0x6C),
            "f18" => Some(0x6D),
            "f19" => Some(0x6E),
            _ => None,
        }
    }

    /// Process a single CGEventTap event on the dedicated processor thread.
    /// Extracted from the old inline callback body so the CGEventTap callback
    /// itself is minimal (capture + channel send).
    fn process_tap_event(
        event: TapEvent,
        recorder: &Arc<Recorder>,
        tray: &TrayIcon,
        app: &AppHandle,
        hotkey_state: &Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
    ) {
        let keycode = event.keycode;
        let flags = event.flags;
        let etype = event.etype;

        // Read current config (RwLock read — nanoseconds, uncontended)
        let (target_keycode, target_flag, controller, cancel_on_esc, cancel_on_hold, fkey_code, is_mouse_key) = {
            let hk = hotkey_state.read();
            let (kc, f) = key_for_name(&hk.key);
            let fkc = fkey_code_for_name(&hk.key);
            (
                kc,
                f,
                Controller::from_mode(&hk.mode, recorder, tray, app),
                hk.cancel_on_esc,
                hk.cancel_on_hold,
                fkc,
                hid_mouse_usage_for_name(&hk.key).is_some(),
            )
        };

        // Escape cancels an active recording, or hides the focused main window
        // while idle. Other focused applications are left untouched.
        if let CGEventType::KeyDown = etype {
            if keycode == ESCAPE_KEYCODE {
                let was_busy = recorder.state().is_busy();
                if cancel_on_esc {
                    controller.cancel_if_busy();
                }
                if !was_busy {
                    if let Some(window) = app.get_webview_window("main") {
                        if window.is_focused().unwrap_or(false) {
                            let _ = window.hide();
                        }
                    }
                }
            }
        }

        if let Some(fkc) = fkey_code {
            // F-key PTT path.
            match etype {
                CGEventType::KeyDown => {
                    if keycode == fkc && !FKEY_DOWN.swap(true, Ordering::AcqRel) {
                        if cancel_on_hold {
                            controller.arm_hold_cancel_if_busy();
                        }
                        controller.press();
                    }
                }
                CGEventType::KeyUp => {
                    if keycode == fkc {
                        FKEY_DOWN.store(false, Ordering::Release);
                        controller.release();
                    }
                }
                _ => {}
            }
        } else if is_mouse_key {
            // Mouse button handled by IOHIDManager raw HID listener
            // (which runs on its own background thread). Skip the
            // CGEventTap so the modifier handler below doesn't
            // accidentally match Right Option (the default fallback
            // in key_for_name) when a mouse button is configured.
        } else if let CGEventType::FlagsChanged = etype {
            // Modifier key PTT path (Right Option, Control, etc.).
            if keycode == target_keycode {
                let is_key_down = flags.contains(target_flag);
                if is_key_down {
                    if cancel_on_hold {
                        controller.arm_hold_cancel_if_busy();
                    }
                    controller.press();
                } else {
                    controller.release();
                }
            }
        }
    }

    pub fn spawn(
        recorder: Arc<Recorder>,
        tray_icon: TrayIcon,
        app: AppHandle,
        hotkey_state: Arc<parking_lot::RwLock<crate::settings::HotkeyConfig>>,
    ) {
        // Start the IOHIDManager mouse listener first — it runs on its own
        // thread and reads raw HID button events regardless of whether
        // Logi Options+ (or similar driver software) intercepts at the
        // IOKit level. Does not require Accessibility trust.
        spawn_hid_mouse_listener(
            recorder.clone(),
            tray_icon.clone(),
            app.clone(),
            hotkey_state.clone(),
        );

        // Start the Input Monitoring permission watchdog — detects runtime
        // revocation (user unchecks TurboTalk in System Settings) and
        // emits a ui-error toast so the user knows why the hotkey stopped.
        spawn_im_watchdog(app.clone());

        // The IOHIDManager handles both mouse buttons and keyboard hotkeys
        // (see hid_mouse_value_callback which checks both K_HIDPAGE_BUTTON
        // and K_HIDPAGE_KEYBOARD). Only Input Monitoring is required.

        std::thread::spawn(move || {
            // Permission watchdog loop. If the process lacks Accessibility trust,
            // emit a toast and poll AXIsProcessTrusted() every 1.5 s; rebuild
            // the tap once trust flips.
            // closure clones happen each iteration because CGEventTap::new
            // consumes the callback regardless of outcome.
            let mut surfaced_permission_error = false;
            // Cap "trusted but tap creation fails" retries so a real OS-level
            // error (HID disabled, sandbox kill, …) doesn't burn CPU forever.
            let mut trusted_failure_retries = 0u32;
            const MAX_TRUSTED_FAILURE_RETRIES: u32 = 6; // ~30 s of 5 s sleeps

            // Create channel for event processing (bounded to 64 to limit
            // callback backpressure).
            let (evt_tx, evt_rx) = bounded::<TapEvent>(64);

            // Spawn processing thread — serializes all PTT events in order
            // so the callback returns near-instantly.
            let proc_recorder = recorder.clone();
            let proc_tray = tray_icon.clone();
            let proc_app = app.clone();
            let proc_hk = hotkey_state.clone();
            std::thread::Builder::new()
                .name("turbotalk-ptt-processor".into())
                .spawn(move || {
                    for event in evt_rx {
                        process_tap_event(event, &proc_recorder, &proc_tray, &proc_app, &proc_hk);
                    }
                })
                .expect("ptt processor thread");

            loop {
                let evt_tx_cb = evt_tx.clone();

                let tap_result = CGEventTap::new(
                    CGEventTapLocation::Session,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    vec![
                        CGEventType::FlagsChanged,
                        CGEventType::KeyDown,
                        CGEventType::KeyUp,
                    ],
                    move |_proxy, etype, event| {
                        // Handle tap-disable events inline — macOS delivers these
                        // on the tap's runloop thread, so re-enable immediately
                        // and return. No polling watchdog needed.
                        match etype {
                            CGEventType::TapDisabledByTimeout
                            | CGEventType::TapDisabledByUserInput => {
                                let raw = TAP_MACH_PORT_RAW.load(Ordering::Acquire)
                                    as *const c_void;
                                if !raw.is_null() {
                                    unsafe { CGEventTapEnable(raw, true) };
                                }
                                return None;
                            }
                            _ => {}
                        }

                        let keycode =
                            event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                        let flags = event.get_flags();
                        let _ = evt_tx_cb.send(TapEvent { keycode, flags, etype });
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
                        // Store raw Mach port pointer so the callback can
                        // re-enable the tap if macOS disables it.
                        TAP_MACH_PORT_RAW.store(
                            tap.mach_port.as_concrete_TypeRef() as usize,
                            Ordering::Release,
                        );
                        let source = match tap.mach_port.create_runloop_source(0) {
                            Ok(s) => s,
                            Err(()) => {
                                tracing::error!(
                                    "[hotkey] CGEventTap create_runloop_source failed — \
                                     retrying tap creation"
                                );
                                // Budget this like a trusted-but-failing retry so we
                                // don't spin forever on a broken Mach port.
                                trusted_failure_retries += 1;
                                if trusted_failure_retries >= MAX_TRUSTED_FAILURE_RETRIES {
                                    tracing::error!(
                                        "[hotkey] giving up after {trusted_failure_retries} \
                                         create_runloop_source failures"
                                    );
                                    return;
                                }
                                std::thread::sleep(std::time::Duration::from_secs(5));
                                continue;
                            }
                        };
                        // SAFETY: kCFRunLoopCommonModes is a static CFStringRef
                        // constant exported by core-foundation. Reading it requires
                        // unsafe because the binding is a static extern, but the
                        // value is immutable and thread-safe to read.
                        CFRunLoop::get_current()
                            .add_source(&source, unsafe { kCFRunLoopCommonModes });
                        tap.enable();

                        // Gate IOHID keyboard handling: while CGEventTap is
                        // active both paths would fire for the same keypress,
                        // causing the immediate-discard race in toggle mode.
                        CGEVENTTAP_ACTIVE.store(true, Ordering::Release);
                        CFRunLoop::run_current();
                        CGEVENTTAP_ACTIVE.store(false, Ordering::Release);
                        return;
                    }
                    Err(()) => {
                        let trusted = accessibility_trusted();
                        if trusted {
                            tracing::error!(
                                "[hotkey] CGEventTap failed (accessibility_trusted={trusted}, retry={trusted_failure_retries})"
                            );
                        } else {
                            tracing::warn!(
                                "[hotkey] CGEventTap unavailable because Accessibility trust is false; IOHID fallback remains active"
                            );
                        }
                        if trusted && !surfaced_permission_error {
                            surfaced_permission_error = true;
                            let kind = "hotkey-input-monitoring";
                            let message = "Record trigger could not receive keyboard events. Turn on Turbo Talk in System Settings → Privacy & Security → Input Monitoring, then restart Turbo Talk.";
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

    #[derive(Debug, Clone, serde::Serialize, specta::Type)]
    pub struct HotkeyProbe {
        pub method: String,
        pub accessibility_trusted: bool,
    }

    pub fn diagnostic_probe() -> HotkeyProbe {
        HotkeyProbe {
            method: "CGEventTap + IOHIDManager".into(),
            accessibility_trusted: accessibility_trusted(),
        }
    }

    pub fn update_hotkey_vk(_key_name: &str, _cancel_on_esc: bool, _cancel_on_hold: bool) {
        // macOS updates hotkey atomics inside its own event tap callback.
        // This is a no-op stub for cross-platform API compatibility.
    }
}

#[cfg(not(target_os = "macos"))]
#[cfg(target_os = "linux")]
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
    use super::Controller;
    use crate::recorder::Recorder;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tauri::{tray::TrayIcon, AppHandle};

    /// True once the rdev listener thread has started; cleared if it exits.
    static LISTENER_ALIVE: AtomicBool = AtomicBool::new(false);

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

    /// Map config key names (shared with the Settings UI) to `rdev::Key` variants.
    ///
    /// macOS uses Right Option (⌥) as the canonical PTT key; on Windows that maps
    /// to VK_RMENU (`Key::AltGr`), which many US-layout keyboards lack as a
    /// distinct physical key. Windows defaults to Right Control instead — see
    /// `HotkeyConfig::default` and `migrate_platform_defaults`.
    fn key_for_name(name: &str) -> Option<rdev::Key> {
        use rdev::Key;
        match name {
            "left_control" => Some(Key::ControlLeft),
            "right_control" => Some(Key::ControlRight),
            "left_command" => Some(Key::MetaLeft),
            "right_command" => Some(Key::MetaRight),
            "left_shift" => Some(Key::ShiftLeft),
            "right_shift" => Some(Key::ShiftRight),
            // Option (macOS) ≡ Alt (Win/X11). Left vs right are distinct VK codes.
            "left_option" => Some(Key::Alt),
            "right_option" => Some(Key::AltGr),
            "numpad_enter" => Some(Key::KpReturn),
            "numpad_0" => Some(Key::Kp0),
            "numpad_decimal" => Some(Key::KpDelete),
            "numpad_add" => Some(Key::KpPlus),
            "numpad_subtract" => Some(Key::KpMinus),
            "numpad_multiply" => Some(Key::KpMultiply),
            unknown => {
                tracing::warn!(
                    "[hotkey] unknown hotkey key {:?} — no rdev mapping",
                    unknown
                );
                None
            }
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
            {
                let hk = hotkey_state.read();
                tracing::info!(
                    "[hotkey] rdev listener starting — key={} mode={}",
                    hk.key,
                    hk.mode
                );
                LISTENER_ALIVE.store(true, Ordering::Release);
            }

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
                let (target_key, controller, cancel_on_esc, cancel_on_hold) = {
                    let hk = hotkey_state.read();
                    (
                        key_for_name(&hk.key),
                        Controller::from_mode(&hk.mode, &recorder, &tray_icon, &app),
                        hk.cancel_on_esc,
                        hk.cancel_on_hold,
                    )
                };
                let Some(target_key) = target_key else {
                    return;
                };

                // Escape → cancel any in-flight recording.
                if let rdev::EventType::KeyPress(rdev::Key::Escape) = event.event_type {
                    if cancel_on_esc {
                        controller.cancel_if_busy();
                        return;
                    }
                }

                match event.event_type {
                    rdev::EventType::KeyPress(key) if key == target_key => {
                        let was_down = down_for_cb.swap(true, Ordering::AcqRel);
                        if was_down {
                            return;
                        }
                        if cancel_on_hold {
                            controller.arm_hold_cancel_if_busy();
                        }
                        controller.press();
                    }
                    rdev::EventType::KeyRelease(key) if key == target_key => {
                        let was_down = down_for_cb.swap(false, Ordering::AcqRel);
                        if !was_down {
                            return;
                        }
                        controller.release();
                    }
                    _ => {}
                }
            });

            if let Err(e) = result {
                LISTENER_ALIVE.store(false, Ordering::Release);
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

    #[derive(Debug, Clone, serde::Serialize, specta::Type)]
    pub struct HotkeyProbe {
        pub method: String,
        pub listener_alive: bool,
        #[cfg(target_os = "linux")]
        pub wayland_detected: bool,
        pub accessibility_trusted: bool,
    }

    pub fn diagnostic_probe() -> HotkeyProbe {
        let method = "rdev XRecord".to_string();
        #[cfg(target_os = "linux")]
        let wayland_detected = std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false);

        HotkeyProbe {
            method,
            listener_alive: LISTENER_ALIVE.load(Ordering::Acquire),
            #[cfg(target_os = "linux")]
            wayland_detected,
            accessibility_trusted: accessibility_trusted(),
        }
    }

    pub fn update_hotkey_vk(_key_name: &str, _cancel_on_esc: bool, _cancel_on_hold: bool) {
        // Linux/X11 updates hotkey atomics inside its own rdev listener.
        // This is a no-op stub for cross-platform API compatibility.
    }
}

#[cfg(target_os = "windows")]
#[path = "hotkey_win32.rs"]
mod hotkey_win32;

#[cfg(target_os = "windows")]
pub use hotkey_win32::{accessibility_trusted, diagnostic_probe, iohid_listener_running, spawn, update_hotkey_vk, HotkeyProbe};
#[cfg(target_os = "macos")]
pub use imp::{accessibility_trusted, diagnostic_probe, iohid_listener_running, spawn, update_hotkey_vk, HotkeyProbe};
#[cfg(target_os = "linux")]
pub use imp::{accessibility_trusted, diagnostic_probe, iohid_listener_running, spawn, update_hotkey_vk, HotkeyProbe};

#[cfg(test)]
mod tests {
    // Controller architecture is validated by compilation and integration
    // testing (the full dictation loop). Isolated unit testing of the
    // HoldController / ToggleController dispatch requires constructed
    // Arc<Recorder> / TrayIcon / AppHandle arguments, which are not
    // available in a unit-test context without heavy mocking infrastructure.
    //
    // Key contracts verified at runtime:
    //   - HoldController::press → common::ptt_down   (always start)
    //   - HoldController::release → common::ptt_up   (always stop)
    //   - ToggleController::press → toggles start/stop
    //   - ToggleController::release → noop
    //   - arm_hold_cancel_if_busy arms suppression only for HoldController
    //   - cancel_if_busy arms suppression only for HoldController
}

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
