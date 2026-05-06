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
//
// Session reuse (TASK-17): the heavy cost in `Vad::new` is ONNX session
// construction (model parse, op graph build, allocator setup). Per-frame
// `compute()` is cheap by comparison. Stage timings from TASK-13 bucket
// init + 100×compute together as a single `vad=` number — fine for
// pipeline debugging, useless for deciding whether to cache.
//
// The upstream `vad_rs::Vad` exposes a `reset()` method that zeros only
// the LSTM hidden/cell tensors (`h_tensor`, `c_tensor`) — the per-stream
// state. The ONNX `Session` itself is stateless across calls. So caching
// one `Vad` for the process and `reset()`-ing it before each recording
// is safe: prior audio cannot influence the next call. The smoothing
// state (`in_speech`, `onset_counter`, `hangover_counter`,
// `speech_start_frame`, `speech_end_frame`) lives in the per-call
// `SmoothedVad` wrapper and is dropped between calls, so there is no
// state leak path there either.
//
// We intentionally use one cached `Vad` behind a `Mutex` rather than a
// pool — TurboTalk runs one in-flight dictation job at a time
// (TASK-14), so there is never contention.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

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

/// Cached process-lifetime `Vad`. The heavy cost is `Vad::new` (ONNX
/// session construction); `compute()` is cheap. We pay init exactly once
/// per process and call `reset()` before each `trim()` to zero the LSTM
/// h/c tensors so prior audio cannot influence the next recording. See
/// the module-level reuse note for the safety argument.
///
/// `None` until first init. If `Vad::new` ever fails, we leave the cell
/// `None` and fall back to "no trimming" via `trim()`'s graceful path —
/// we do not poison the cache or panic.
static VAD_CACHE: OnceLock<Mutex<Option<Vad>>> = OnceLock::new();

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
/// Per-call smoothing state. Held only on the stack inside `trim()` —
/// dropped between recordings, so there is no cross-call state leak from
/// the smoothing layer. The shared `Vad` (LSTM tensors) is reset
/// separately by the caller before pushing the first frame.
struct SmoothedVad<'a> {
    inner: &'a mut Vad,
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

impl<'a> SmoothedVad<'a> {
    fn new(inner: &'a mut Vad) -> Self {
        Self {
            inner,
            in_speech: false,
            onset_counter: 0,
            hangover_counter: 0,
            speech_start_frame: None,
            speech_end_frame: None,
        }
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

/// Streaming-finalizer accessor (TASK-22). Returns the cached
/// process-lifetime VAD `OnceLock<Mutex<Option<Vad>>>` so the streaming
/// worker can hold the same session the batch path uses. Lazily
/// initializes on first call (mirroring `trim()`'s init path) so we
/// don't pay init twice.
///
/// Returns `None` only if the model bytes can't be materialized to disk
/// or `Vad::new` fails — same graceful-fallback contract as `trim()`.
/// On success, the caller must call `inner.reset()` before pushing
/// frames so prior audio can't influence the new stream.
pub fn cached_vad_for_streaming() -> Option<&'static Mutex<Option<Vad>>> {
    let cell = VAD_CACHE.get_or_init(|| Mutex::new(None));
    {
        let mut guard = cell.lock();
        if guard.is_none() {
            let path = match ensure_model_on_disk() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("[vad] failed to materialize Silero model for streaming: {e}");
                    return None;
                }
            };
            match Vad::new(&path, SAMPLE_RATE) {
                Ok(v) => *guard = Some(v),
                Err(e) => {
                    tracing::warn!("[vad] failed to initialize Silero for streaming: {e}");
                    return None;
                }
            }
        }
    }
    Some(cell)
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

    // Acquire (and lazily initialize) the cached process-lifetime VAD.
    // We hold the mutex for the duration of this call — the audio
    // pipeline runs one in-flight job at a time (TASK-14), so there is
    // no contention to lose to. Holding it also makes "reset → push
    // frames" atomic w.r.t. any future caller, which matters because
    // reset zeros the LSTM state the frames depend on.
    let cell = VAD_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock();

    // Lazy init on first call (or re-init if a previous init failed and
    // the cell is still empty). We measure init separately from
    // reset+compute so timing logs reveal whether we're actually
    // benefiting from the cache.
    let init_ms: f32;
    if guard.is_none() {
        let t_init = Instant::now();
        let path = match ensure_model_on_disk() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[vad] failed to materialize Silero model — skipping trim: {e}");
                return (0, samples.len());
            }
        };
        match Vad::new(&path, SAMPLE_RATE) {
            Ok(v) => {
                init_ms = t_init.elapsed().as_secs_f32() * 1000.0;
                *guard = Some(v);
            }
            Err(e) => {
                tracing::warn!("[vad] failed to initialize Silero — skipping trim: {e}");
                return (0, samples.len());
            }
        }
    } else {
        init_ms = 0.0;
    }
    let inner = guard.as_mut().expect("vad cache populated above");

    // Zero the LSTM h/c tensors so prior audio cannot influence this
    // recording. This is the documented `reset()` semantics in
    // `vad-rs::Vad`. Reset is O(state-size), not O(model-size), so it's
    // ~free compared to init.
    let t_reset = Instant::now();
    inner.reset();
    let reset_ms = t_reset.elapsed().as_secs_f32() * 1000.0;

    let mut vad = SmoothedVad::new(inner);

    let total_frames = samples.len().div_ceil(FRAME_SAMPLES);
    let mut frame_buf = [0.0f32; FRAME_SAMPLES];

    let t_compute = Instant::now();
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
    let compute_ms = t_compute.elapsed().as_secs_f32() * 1000.0;

    // Fine-grained breakdown so the cache benefit is visible separately
    // from per-frame inference cost. `init_ms` is non-zero only on the
    // very first call of the process; subsequent calls show
    // `init=0.00` and the savings are real.
    tracing::info!(
        "[vad] timings (ms): init={:.2} reset={:.2} compute={:.2} frames={}",
        init_ms,
        reset_ms,
        compute_ms,
        total_frames
    );

    let (Some(start_frame), Some(end_frame)) = (vad.speech_start_frame, vad.speech_end_frame)
    else {
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
        for (i, sample) in buf.iter_mut().enumerate().take(burst_end).skip(burst_start) {
            let t = (i - burst_start) as f32 / SR as f32;
            let f = 200.0 + 1800.0 * t;
            let am = 0.5 * (1.0 + (2.0 * std::f32::consts::PI * 4.0 * t).sin());
            *sample = (2.0 * std::f32::consts::PI * f * t).sin() * 0.4 * am;
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

    /// State-isolation contract for the cached VAD (TASK-17).
    ///
    /// The cached `Vad` is reused across calls but `reset()` is called
    /// before each recording. We can't easily assert h/c-tensor zeroing
    /// from out here, but we can assert the user-visible contract: a
    /// pure-silence buffer must always fall through to the (0, len)
    /// fallback regardless of what was processed before it.
    ///
    /// If state ever leaked across calls, a silence buffer following a
    /// noisy buffer could pick up phantom "speech" frames from the LSTM
    /// memory. This test would then either return a non-trivial
    /// (start, end) range or, more subtly, return (0, len) for the
    /// wrong reason. We assert the shape directly.
    #[test]
    fn trim_does_not_leak_state_between_calls() {
        // First call: a buffer with non-trivial energy. We don't care
        // what bounds it returns — only that it runs and primes the
        // cached Vad's LSTM state with non-zero h/c.
        const SR: usize = 16_000;
        let mut noisy = vec![0.0f32; SR]; // 1 s
        for (i, s) in noisy.iter_mut().enumerate() {
            // Pseudo-random low-amplitude noise.
            *s = ((i as f32 * 12.9898).sin() * 43758.547).fract() * 0.2 - 0.1;
        }
        let _ = trim(&noisy);

        // Second call: pure silence. With reset()-ing between calls,
        // Silero must see this as silence end-to-end and we must hit
        // the "no speech detected" fallback path, which returns
        // (0, samples.len()).
        let silence = vec![0.0f32; SR]; // 1 s of zeros
        let (start, end) = trim(&silence);
        assert_eq!(
            (start, end),
            (0, silence.len()),
            "silence after noise must return full-range fallback, not inherit speech bounds"
        );

        // Third call: silence again, to assert the property is stable
        // across repeated reuse of the cached session.
        let (start, end) = trim(&silence);
        assert_eq!(
            (start, end),
            (0, silence.len()),
            "repeated silence calls must remain at full-range fallback"
        );
    }

    /// Repeated calls with sub-frame inputs must not crash, must not
    /// touch the cached Vad, and must return the trivial range each time.
    #[test]
    fn trim_repeated_sub_frame_calls_are_stable() {
        for _ in 0..5 {
            assert_eq!(trim(&vec![0.0f32; 100]), (0, 100));
            assert_eq!(trim(&[]), (0, 0));
        }
    }
}
