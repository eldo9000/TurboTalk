use crate::audio::{AudioCapture, StopOutcome};
use parking_lot::Mutex;
use thiserror::Error;

/// Authoritative dictation-job lifecycle.
///
///   `Ready → Recording → FinalizingAudio → Transcribing → Cleaning → Pasting → Ready`
///
/// One in-flight job total. The hotkey path refuses to start a new recording
/// unless the recorder is in `Ready`. Any error or discard along the way
/// returns the recorder to `Ready` so a single failure can never pin the
/// state machine outside `Ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ready,
    Recording,
    FinalizingAudio,
    Transcribing,
    Cleaning,
    Pasting,
}

impl State {
    fn as_str(self) -> &'static str {
        match self {
            State::Ready => "Ready",
            State::Recording => "Recording",
            State::FinalizingAudio => "FinalizingAudio",
            State::Transcribing => "Transcribing",
            State::Cleaning => "Cleaning",
            State::Pasting => "Pasting",
        }
    }

    /// True for every state except `Ready`. The hotkey uses this to decide
    /// whether a fresh `start()` is allowed or the press should be reported
    /// as `dictation-busy` instead.
    pub fn is_busy(self) -> bool {
        !matches!(self, State::Ready)
    }
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("illegal transition: cannot {attempted} while in {from}")]
    IllegalTransition {
        from: State,
        attempted: &'static str,
    },
    #[error("audio error: {0}")]
    Audio(#[from] anyhow::Error),
}

pub struct Recorder {
    capture: AudioCapture,
    state: Mutex<State>,
}

impl Recorder {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            capture: AudioCapture::new()?,
            state: Mutex::new(State::Ready),
        })
    }

    /// Snapshot the current lifecycle state. Cheap (single mutex lock).
    pub fn state(&self) -> State {
        *self.state.lock()
    }

    pub fn start(&self) -> Result<(), RecorderError> {
        let mut s = self.state.lock();
        if !matches!(*s, State::Ready) {
            return Err(RecorderError::IllegalTransition {
                from: *s,
                attempted: "start",
            });
        }
        self.capture.start()?;
        *s = State::Recording;
        tracing::info!("[recorder] Ready → Recording");
        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        matches!(*self.state.lock(), State::Recording)
    }

    pub fn level(&self) -> f32 {
        self.capture.level()
    }

    /// Edge-triggered: true exactly once when the cpal error callback flagged
    /// the device as gone. The level-broadcast thread polls this every tick.
    pub fn device_lost(&self) -> bool {
        self.capture.device_lost()
    }

    /// Stop capture and run the audio finalization pipeline (downmix /
    /// resample / VAD / normalize / WAV write).
    ///
    /// Transitions: `Recording → FinalizingAudio` for the duration of the
    /// pipeline, then either:
    ///   - leaves the recorder in `FinalizingAudio` on `Ok(StopOutcome::Wav)`
    ///     so the caller can advance through the rest of the lifecycle via
    ///     `begin_transcribing` → `begin_cleaning` → `begin_pasting` → `finish`;
    ///   - drops back to `Ready` on `Ok(StopOutcome::Discard)` (no transcription
    ///     to do) and on `Err` (audio pipeline failed — short-circuit the job).
    pub fn stop(&self) -> Result<StopOutcome, RecorderError> {
        let mut s = self.state.lock();
        if !matches!(*s, State::Recording) {
            return Err(RecorderError::IllegalTransition {
                from: *s,
                attempted: "stop",
            });
        }
        *s = State::FinalizingAudio;
        drop(s);
        tracing::info!("[recorder] Recording → FinalizingAudio");

        let outcome = self.capture.stop();
        match &outcome {
            Ok(StopOutcome::Wav { .. }) => {
                // Stay in FinalizingAudio; the caller advances through
                // Transcribing / Cleaning / Pasting and eventually `finish()`.
            }
            Ok(StopOutcome::Discard(_)) => {
                *self.state.lock() = State::Ready;
                tracing::info!("[recorder] FinalizingAudio → Ready (discarded)");
            }
            Err(_) => {
                // Always transition back to Ready, even if the capture-stop
                // pipeline errored. Otherwise a transient audio failure pins
                // the recorder in FinalizingAudio forever and every subsequent
                // hotkey press is rejected as IllegalTransition.
                *self.state.lock() = State::Ready;
                tracing::info!("[recorder] FinalizingAudio → Ready (error)");
            }
        }
        outcome.map_err(Into::into)
    }

    /// Advance: `FinalizingAudio → Transcribing`. Called by the hotkey path
    /// once the WAV is on disk and Whisper is about to run.
    pub fn begin_transcribing(&self) -> Result<(), RecorderError> {
        self.transition(
            "begin_transcribing",
            State::FinalizingAudio,
            State::Transcribing,
        )
    }

    /// Advance: `Transcribing → Cleaning`. Called once Whisper returns text
    /// and the cleanup / chaperone pass is about to run.
    pub fn begin_cleaning(&self) -> Result<(), RecorderError> {
        self.transition("begin_cleaning", State::Transcribing, State::Cleaning)
    }

    /// Advance: `Cleaning → Pasting`. Called once cleanup is done and we are
    /// about to inject text into the focused app.
    pub fn begin_pasting(&self) -> Result<(), RecorderError> {
        self.transition("begin_pasting", State::Cleaning, State::Pasting)
    }

    /// End of the lifecycle from any non-Ready stage: `* → Ready`.
    /// Idempotent so error / cleanup paths can call it without first
    /// checking the current state.
    pub fn finish(&self) {
        let mut s = self.state.lock();
        if !matches!(*s, State::Ready) {
            tracing::info!("[recorder] {} → Ready (finish)", *s);
            *s = State::Ready;
        }
    }

    fn transition(
        &self,
        attempted: &'static str,
        expected: State,
        next: State,
    ) -> Result<(), RecorderError> {
        let mut s = self.state.lock();
        if *s != expected {
            return Err(RecorderError::IllegalTransition {
                from: *s,
                attempted,
            });
        }
        tracing::info!("[recorder] {} → {}", *s, next);
        *s = next;
        Ok(())
    }

    /// Force the recorder back to Ready without producing a WAV. Used by the
    /// level-broadcast thread when device loss is detected — there's no
    /// hotkey-up coming, so we synthesize the cleanup ourselves.
    ///
    /// TASK-23: also called by the cancel-gesture path (Ctrl+Alt hold or Esc).
    ///   - From `Recording`: drops the audio stream; no WAV produced.
    ///   - From `Transcribing`: additionally kills the active whisper-cli
    ///     subprocess via `transcribe::abort_active()`.
    ///   - From any other state: debug-logged, no-op (idempotent).
    ///
    /// Idempotent: calling cancel() while in Ready is a no-op.
    pub fn cancel(&self) {
        let mut s = self.state.lock();
        match *s {
            State::Ready => {
                tracing::debug!("[recorder] cancel() called while already Ready — no-op");
            }
            State::Transcribing => {
                // Kill the in-flight whisper-cli subprocess (best-effort).
                crate::transcribe::abort_active();
                self.capture.cancel();
                tracing::info!("[recorder] Transcribing → Ready (user cancelled)");
                *s = State::Ready;
            }
            State::Recording | State::FinalizingAudio | State::Cleaning | State::Pasting => {
                self.capture.cancel();
                tracing::info!("[recorder] {} → Ready (cancelled)", *s);
                *s = State::Ready;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a Recorder whose AudioCapture is never actually started.
    /// We exercise the state machine by manipulating `state` directly and by
    /// calling the transition helpers — the underlying capture stays idle.
    fn fresh() -> Recorder {
        Recorder::new().expect("AudioCapture::new")
    }

    #[test]
    fn state_starts_ready() {
        let r = fresh();
        assert_eq!(r.state(), State::Ready);
        assert!(!r.state().is_busy());
        assert!(!r.is_recording());
    }

    #[test]
    fn is_busy_true_for_every_non_ready_state() {
        for s in [
            State::Recording,
            State::FinalizingAudio,
            State::Transcribing,
            State::Cleaning,
            State::Pasting,
        ] {
            assert!(s.is_busy(), "{:?} should be busy", s);
        }
        assert!(!State::Ready.is_busy());
    }

    /// `start()` is illegal from anything other than `Ready`. We don't call
    /// the real `start()` here (which would open a cpal stream) — instead we
    /// pin the state to a busy value and verify the guard rejects.
    #[test]
    fn start_rejected_when_busy() {
        for busy in [
            State::Recording,
            State::FinalizingAudio,
            State::Transcribing,
            State::Cleaning,
            State::Pasting,
        ] {
            let r = fresh();
            *r.state.lock() = busy;
            let err = r.start().expect_err("start should fail when busy");
            match err {
                RecorderError::IllegalTransition { from, attempted } => {
                    assert_eq!(from, busy);
                    assert_eq!(attempted, "start");
                }
                other => panic!("expected IllegalTransition, got {:?}", other),
            }
            // State must be unchanged — the guard rejects without side effect.
            assert_eq!(r.state(), busy);
        }
    }

    #[test]
    fn stop_rejected_when_not_recording() {
        for not_recording in [
            State::Ready,
            State::FinalizingAudio,
            State::Transcribing,
            State::Cleaning,
            State::Pasting,
        ] {
            let r = fresh();
            *r.state.lock() = not_recording;
            // `StopOutcome` doesn't implement `Debug`, so we can't use
            // `expect_err`. Match the result manually instead.
            match r.stop() {
                Err(RecorderError::IllegalTransition { from, attempted }) => {
                    assert_eq!(from, not_recording);
                    assert_eq!(attempted, "stop");
                }
                Err(other) => panic!("expected IllegalTransition, got {:?}", other),
                Ok(_) => panic!("expected Err for stop() in {:?}", not_recording),
            }
            assert_eq!(r.state(), not_recording);
        }
    }

    #[test]
    fn begin_transcribing_only_from_finalizing_audio() {
        // Legal: FinalizingAudio → Transcribing.
        let r = fresh();
        *r.state.lock() = State::FinalizingAudio;
        r.begin_transcribing().expect("legal transition");
        assert_eq!(r.state(), State::Transcribing);

        // Illegal from every other state.
        for from in [
            State::Ready,
            State::Recording,
            State::Transcribing,
            State::Cleaning,
            State::Pasting,
        ] {
            let r = fresh();
            *r.state.lock() = from;
            let err = r.begin_transcribing().expect_err("illegal");
            match err {
                RecorderError::IllegalTransition { from: f, attempted } => {
                    assert_eq!(f, from);
                    assert_eq!(attempted, "begin_transcribing");
                }
                other => panic!("expected IllegalTransition, got {:?}", other),
            }
            assert_eq!(r.state(), from);
        }
    }

    #[test]
    fn begin_cleaning_only_from_transcribing() {
        let r = fresh();
        *r.state.lock() = State::Transcribing;
        r.begin_cleaning().expect("legal transition");
        assert_eq!(r.state(), State::Cleaning);

        for from in [
            State::Ready,
            State::Recording,
            State::FinalizingAudio,
            State::Cleaning,
            State::Pasting,
        ] {
            let r = fresh();
            *r.state.lock() = from;
            assert!(r.begin_cleaning().is_err());
            assert_eq!(r.state(), from);
        }
    }

    #[test]
    fn begin_pasting_only_from_cleaning() {
        let r = fresh();
        *r.state.lock() = State::Cleaning;
        r.begin_pasting().expect("legal transition");
        assert_eq!(r.state(), State::Pasting);

        for from in [
            State::Ready,
            State::Recording,
            State::FinalizingAudio,
            State::Transcribing,
            State::Pasting,
        ] {
            let r = fresh();
            *r.state.lock() = from;
            assert!(r.begin_pasting().is_err());
            assert_eq!(r.state(), from);
        }
    }

    #[test]
    fn finish_returns_to_ready_from_any_state() {
        for from in [
            State::Ready,
            State::Recording,
            State::FinalizingAudio,
            State::Transcribing,
            State::Cleaning,
            State::Pasting,
        ] {
            let r = fresh();
            *r.state.lock() = from;
            r.finish();
            assert_eq!(r.state(), State::Ready, "finish() from {:?}", from);
        }
    }

    #[test]
    fn finish_is_idempotent_in_ready() {
        let r = fresh();
        r.finish();
        r.finish();
        assert_eq!(r.state(), State::Ready);
    }

    #[test]
    fn cancel_returns_to_ready_from_every_busy_state() {
        for from in [
            State::Recording,
            State::FinalizingAudio,
            State::Transcribing,
            State::Cleaning,
            State::Pasting,
        ] {
            let r = fresh();
            *r.state.lock() = from;
            r.cancel();
            assert_eq!(r.state(), State::Ready, "cancel() from {:?}", from);
        }
    }

    /// Walking the full happy path through the transition helpers must end
    /// in Ready.
    #[test]
    fn full_lifecycle_round_trip() {
        let r = fresh();
        // Manually pin to FinalizingAudio (we can't run the real audio
        // pipeline in a unit test).
        *r.state.lock() = State::FinalizingAudio;
        r.begin_transcribing().expect("ok");
        r.begin_cleaning().expect("ok");
        r.begin_pasting().expect("ok");
        r.finish();
        assert_eq!(r.state(), State::Ready);
    }
}
