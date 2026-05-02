use crate::audio::{AudioCapture, StopOutcome};
use parking_lot::Mutex;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ready,
    Recording,
    Transcribing,
}

impl State {
    fn as_str(self) -> &'static str {
        match self {
            State::Ready => "Ready",
            State::Recording => "Recording",
            State::Transcribing => "Transcribing",
        }
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

    pub fn stop(&self) -> Result<StopOutcome, RecorderError> {
        let mut s = self.state.lock();
        if !matches!(*s, State::Recording) {
            return Err(RecorderError::IllegalTransition {
                from: *s,
                attempted: "stop",
            });
        }
        *s = State::Transcribing;
        drop(s);
        tracing::info!("[recorder] Recording → Transcribing");

        // Always transition back to Ready, even if the capture-stop pipeline
        // (resample / VAD / normalize / WAV-write) errored. Otherwise a
        // transient audio failure pins the recorder in Transcribing forever
        // and every subsequent hotkey press is rejected as IllegalTransition.
        let outcome = self.capture.stop();
        *self.state.lock() = State::Ready;
        tracing::info!("[recorder] Transcribing → Ready");
        outcome.map_err(Into::into)
    }

    /// Force the recorder back to Ready without producing a WAV. Used by the
    /// level-broadcast thread when device loss is detected — there's no
    /// hotkey-up coming, so we synthesize the cleanup ourselves.
    ///
    /// Idempotent: calling cancel() while in Ready is a no-op.
    pub fn cancel(&self) {
        let mut s = self.state.lock();
        match *s {
            State::Ready => (),
            State::Recording | State::Transcribing => {
                self.capture.cancel();
                *s = State::Ready;
                tracing::info!("[recorder] cancelled → Ready");
            }
        }
    }
}
