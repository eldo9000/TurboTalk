use crate::audio::AudioCapture;
use parking_lot::Mutex;
use std::path::PathBuf;
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

    pub fn stop(&self) -> Result<Option<PathBuf>, RecorderError> {
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
        let path_opt = self.capture.stop()?;
        *self.state.lock() = State::Ready;
        tracing::info!("[recorder] Transcribing → Ready");
        Ok(path_opt)
    }
}
