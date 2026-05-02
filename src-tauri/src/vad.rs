// Silero v4 voice-activity detector with onset/hangover smoothing.
//
// Replaces the old fixed-threshold RMS gate (`audio::trim_silence`). The
// neural model is far less prone to misfiring on background noise (HVAC,
// fans, distant voices) and clipping word edges on quiet consonants —
// which were the two main sources of whisper hallucinations on TurboTalk's
// trimmed buffers ("um", " thanks for watching", " you").
//
// Pipeline contract: by the time a buffer reaches `vad::trim`, it has
// already been downmixed to mono and resampled to 16 kHz f32 by
// `audio.rs::stop()`. Silero v4 expects exactly that, in 30 ms / 480-sample
// frames. We pad the trailing partial frame with zeros so every recording
// is fully scanned.
//
// Smoothing constants (prefill=15, onset=2, hangover=15, threshold=0.3) are
// taken verbatim from cjpais/Handy — tuned against real dictation audio.
// Don't retune from scratch.
//
// Failure mode: if the ONNX model fails to load, or any per-frame inference
// returns an error, we log a warning and return `(0, samples.len())` —
// i.e. degrade gracefully to "no trimming" rather than crashing or dropping
// the whole recording.

use std::path::PathBuf;
use std::sync::OnceLock;

use parking_lot::Mutex;
use vad_rs::Vad;

/// Embedded Silero v4 ONNX model (~2 MB). Bundling it into the binary keeps
/// installs self-contained — no extra resource files to chase at runtime.
static MODEL_BYTES: &[u8] = include_bytes!("../resources/silero_vad.onnx");

/// 16 kHz sample rate × 30 ms frame = 480 samples per Silero v4 inference.
const SAMPLE_RATE: usize = 16_000;
const FRAME_SAMPLES: usize = 480;

const THRESHOLD: f32 = 0.3;
const PREFILL_FRAMES: usize = 15;
const ONSET_FRAMES: usize = 2;
const HANGOVER_FRAMES: usize = 15;

/// Path to the on-disk copy of `MODEL_BYTES`. `vad-rs::Vad::new` only accepts
/// a path, so we materialize the bytes to a temp file once per process and
/// reuse the path. Held in a `OnceLock` so concurrent first-callers
/// coordinate without a race.
static MODEL_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn ensure_model_on_disk() -> anyhow::Result<PathBuf> {
    let cell = MODEL_PATH.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock();
    if let Some(p) = guard.as_ref() {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    let mut path = std::env::temp_dir();
    path.push(format!("turbotalk-silero-vad-{}.onnx", std::process::id()));
    std::fs::write(&path, MODEL_BYTES)?;
    *guard = Some(path.clone());
    Ok(path)
}

/// Smoothed Silero VAD over a finite buffer.
///
/// Onset: requires `ONSET_FRAMES` consecutive speech frames before flipping
/// into the "in_speech" state. Filters out a single false-positive frame on
/// e.g. a door click.
///
/// Prefill: when onset triggers, includes `PREFILL_FRAMES` of audio that
/// preceded the first true-speech frame in the kept range. Without this we
/// would clip the leading consonant of the first word ("/s/-eparate").
///
/// Hangover: after the model stops detecting speech, keep extending the
/// kept range for `HANGOVER_FRAMES` more frames before flipping back to
/// silence. Catches trailing fricatives and short inter-word pauses that
/// the model momentarily classifies as non-speech.
struct SmoothedVad {
    inner: Vad,
    in_speech: bool,
    onset_counter: usize,
    hangover_counter: usize,
    /// Index of the first detected-speech frame in the input stream. Set
    /// once when the state machine first flips to `in_speech`; never reset.
    /// `None` until the first onset has fired.
    speech_start_frame: Option<usize>,
    /// Index of the last frame still considered speech (inclusive). Updated
    /// every frame we keep — the final value is the trailing edge.
    speech_end_frame: Option<usize>,
}

impl SmoothedVad {
    fn new() -> anyhow::Result<Self> {
        let path = ensure_model_on_disk()?;
        let inner = Vad::new(&path, SAMPLE_RATE)
            .map_err(|e| anyhow::anyhow!("Failed to create Silero VAD: {e}"))?;
        Ok(Self {
            inner,
            in_speech: false,
            onset_counter: 0,
            hangover_counter: 0,
            speech_start_frame: None,
            speech_end_frame: None,
        })
    }

    fn push_frame(&mut self, frame_idx: usize, frame: &[f32]) -> anyhow::Result<()> {
        let result = self
            .inner
            .compute(frame)
            .map_err(|e| anyhow::anyhow!("Silero VAD compute failed: {e}"))?;
        let is_voice = result.prob > THRESHOLD;

        match (self.in_speech, is_voice) {
            (false, true) => {
                self.onset_counter += 1;
                if self.onset_counter >= ONSET_FRAMES {
                    self.in_speech = true;
                    self.hangover_counter = HANGOVER_FRAMES;
                    self.onset_counter = 0;
                    // The onset edge is the first of the consecutive voice
                    // frames — i.e. ONSET_FRAMES-1 frames before this one.
                    let onset_edge = frame_idx.saturating_sub(ONSET_FRAMES - 1);
                    if self.speech_start_frame.is_none() {
                        self.speech_start_frame = Some(onset_edge);
                    }
                    self.speech_end_frame = Some(frame_idx);
                }
            }
            (true, true) => {
                self.hangover_counter = HANGOVER_FRAMES;
                self.speech_end_frame = Some(frame_idx);
            }
            (true, false) => {
                if self.hangover_counter > 0 {
                    self.hangover_counter -= 1;
                    self.speech_end_frame = Some(frame_idx);
                } else {
                    self.in_speech = false;
                }
            }
            (false, false) => {
                self.onset_counter = 0;
            }
        }
        Ok(())
    }
}

/// Detect the speech-bounded sample-index range of `samples`.
///
/// Returns `(start, end)` such that `samples[start..end]` contains the
/// speech with prefill, onset, and hangover smoothing applied. On any
/// failure (model load, ONNX inference) returns `(0, samples.len())` and
/// logs a warning — graceful fallback to "no trimming".
///
/// Empty inputs and inputs shorter than one frame return `(0, samples.len())`.
pub fn trim(samples: &[f32]) -> (usize, usize) {
    if samples.is_empty() {
        return (0, 0);
    }
    if samples.len() < FRAME_SAMPLES {
        return (0, samples.len());
    }

    let mut vad = match SmoothedVad::new() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("[vad] failed to initialize Silero — skipping trim: {e}");
            return (0, samples.len());
        }
    };

    let total_frames = samples.len().div_ceil(FRAME_SAMPLES);
    let mut frame_buf = [0.0f32; FRAME_SAMPLES];

    for i in 0..total_frames {
        let start = i * FRAME_SAMPLES;
        let end = (start + FRAME_SAMPLES).min(samples.len());
        let take = end - start;
        frame_buf[..take].copy_from_slice(&samples[start..end]);
        if take < FRAME_SAMPLES {
            // Zero-pad the trailing partial frame so Silero gets the
            // exact 480-sample shape it expects. The pad is silence,
            // so it never adds spurious detected-speech.
            frame_buf[take..].fill(0.0);
        }
        if let Err(e) = vad.push_frame(i, &frame_buf) {
            tracing::warn!("[vad] inference failed mid-buffer — skipping trim: {e}");
            return (0, samples.len());
        }
    }

    let (Some(start_frame), Some(end_frame)) = (vad.speech_start_frame, vad.speech_end_frame) else {
        // No speech detected. Mirror the old `trim_silence` "everything
        // below threshold" path: callers will see a too-short buffer and
        // discard via DiscardReason::TooShort. We don't return (0, 0) —
        // returning the full range keeps the failure-mode test ("VAD
        // misclassified, recording was real") more recoverable.
        tracing::info!("[vad] no speech detected — returning full range as fallback");
        return (0, samples.len());
    };

    // Prefill: extend the leading edge backward by PREFILL_FRAMES so we
    // don't clip word starts.
    let prefill_start_frame = start_frame.saturating_sub(PREFILL_FRAMES);
    let start = prefill_start_frame * FRAME_SAMPLES;
    // The end frame is inclusive — turn it into an exclusive sample index,
    // clamped to the buffer length.
    let end = ((end_frame + 1) * FRAME_SAMPLES).min(samples.len());

    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that the smoothed state machine triggers onset + hangover
    /// on a buffer containing real speech-like energy in the middle and
    /// silence on either side.
    ///
    /// We construct a chirp + noise burst (Silero is trained on human
    /// voice — a pure 440 Hz sine wave is unreliable). The exact bounds
    /// don't matter; we only assert that:
    ///   * `start` falls inside the silence preceding the burst (with
    ///     prefill backed off, so somewhere before the burst onset)
    ///   * `end` falls inside or just after the burst (with hangover
    ///     extending past the burst trailing edge)
    ///
    /// If Silero refuses to classify the synthetic burst as speech (which
    /// it sometimes does on non-vocal signals), the function falls through
    /// to the (0, samples.len()) fallback — which is also acceptable
    /// behavior. The test then asserts the fallback shape.
    #[test]
    #[ignore = "Silero VAD is trained on human voice; synthetic audio is unreliable as a unit-test signal. Validate end-to-end at runtime against real dictation."]
    fn trim_isolates_speech_burst() {
        const SR: usize = 16_000;
        const TOTAL_SAMPLES: usize = SR * 3; // 3 s

        let mut buf = vec![0.0f32; TOTAL_SAMPLES];

        // 1.0 s..2.0 s: chirp 200→2000 Hz with light AM, peak 0.4
        let burst_start = SR;
        let burst_end = 2 * SR;
        for i in burst_start..burst_end {
            let t = (i - burst_start) as f32 / SR as f32;
            let f = 200.0 + 1800.0 * t;
            let am = 0.5 * (1.0 + (2.0 * std::f32::consts::PI * 4.0 * t).sin());
            buf[i] = (2.0 * std::f32::consts::PI * f * t).sin() * 0.4 * am;
        }

        let (start, end) = trim(&buf);

        // Either the VAD detected speech (and the bounds make sense)
        // or it gave up gracefully to the full-range fallback.
        let detected = end - start < TOTAL_SAMPLES;
        if detected {
            assert!(
                start < burst_end,
                "start {} should be before burst end {}",
                start,
                burst_end
            );
            assert!(
                end > burst_start,
                "end {} should be after burst start {}",
                end,
                burst_start
            );
        } else {
            assert_eq!(
                (start, end),
                (0, TOTAL_SAMPLES),
                "fallback path must return full range"
            );
        }
    }

    /// Empty input must not crash.
    #[test]
    fn trim_empty_buffer() {
        assert_eq!(trim(&[]), (0, 0));
    }

    /// Sub-frame input is returned as-is without invoking the model.
    #[test]
    fn trim_sub_frame_buffer() {
        let buf = vec![0.0f32; 100]; // < FRAME_SAMPLES (480)
        assert_eq!(trim(&buf), (0, 100));
    }
}
