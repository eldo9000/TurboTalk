use crate::audio::AudioCapture;
use parking_lot::Mutex;
use std::path::PathBuf;

enum State {
    Ready,
    Recording,
    Transcribing,
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

    pub fn start(&self) -> anyhow::Result<()> {
        let mut s = self.state.lock();
        if matches!(*s, State::Ready) {
            self.capture.start();
            *s = State::Recording;
            tracing::info!("[recorder] Ready → Recording");
        }
        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        matches!(*self.state.lock(), State::Recording)
    }

    pub fn stop(&self) -> anyhow::Result<Option<PathBuf>> {
        let mut s = self.state.lock();
        if matches!(*s, State::Recording) {
            *s = State::Transcribing;
            drop(s);
            tracing::info!("[recorder] Recording → Transcribing");
            let path = self.capture.stop()?;
            *self.state.lock() = State::Ready;
            tracing::info!("[recorder] Transcribing → Ready");
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }
}
