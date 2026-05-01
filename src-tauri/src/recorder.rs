use crate::audio::{self, ActiveRecording};
use parking_lot::Mutex;
use std::path::PathBuf;

enum State {
    Ready,
    Recording(ActiveRecording),
    Transcribing,
}

pub struct Recorder {
    state: Mutex<State>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State::Ready),
        }
    }

    pub fn start(&self) -> anyhow::Result<()> {
        let mut s = self.state.lock();
        if matches!(*s, State::Ready) {
            *s = State::Recording(audio::start()?);
            tracing::info!("[recorder] Ready → Recording");
        }
        Ok(())
    }

    pub fn stop(&self) -> anyhow::Result<Option<PathBuf>> {
        let mut s = self.state.lock();
        let prev = std::mem::replace(&mut *s, State::Transcribing);
        if let State::Recording(rec) = prev {
            drop(s);
            tracing::info!("[recorder] Recording → Transcribing");
            let path = audio::stop(rec)?;
            *self.state.lock() = State::Ready;
            tracing::info!("[recorder] Transcribing → Ready");
            Ok(Some(path))
        } else {
            *s = prev;
            Ok(None)
        }
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}
