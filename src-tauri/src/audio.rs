// Lazy pre-warm:
//   AudioCapture keeps the cpal stream open between recordings, with the
//   `is_recording` flag inside the cpal callback gating whether captured
//   samples are retained. After `stop()` / `cancel()`, the stream stays
//   warm for `IDLE_TIMEOUT`; a watchdog thread closes it once the idle
//   deadline elapses so the macOS mic indicator clears when the user is
//   not actively dictating. Back-to-back recordings within the idle
//   window skip CoreAudio init entirely.
//
//   Device change handling: each `start()` re-reads the configured device
//   from settings; if it differs from the warm stream's device the warm
//   stream is dropped and a new one is opened. Built-in ↔ AirPods still
//   works at the cost of a single cold-start latency on the first press
//   after the swap.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tempfile::TempPath;

use crate::audio_finalizer::{DropReason, FinalizeResult, SegmentEmit, StreamingFinalizer};
use crossbeam_channel::Receiver as SegReceiver;

/// Ring buffer capacity for raw capture samples (~5s at 48 kHz stereo f32).
const RING_CAPACITY: usize = 480_000;

/// Peak target ≈ -1 dBFS — leaves 1 dB of headroom so the i16 conversion
/// can't clip even on the loudest inter-sample peaks. Matches Handy and
/// other reference dictation apps.
const NORMALIZE_PEAK: f32 = 0.89;

// ---- Audio pipeline contract (post-release stages) -----------------------
// The on-disk handoff to whisper-cli is *always* the spec below: 16 kHz mono
// 16-bit PCM WAV, no compression. These constants are the single source of
// truth for the resampler target, the VAD/min-duration math, and the WAV
// writer spec. Do not hardcode 16_000 / 1 / 16 in stop(); use these.
//
// See ARCHITECTURE.md → "Audio Pipeline Contract" for the full ordering and
// the reason silence trimming happens *after* resample (Silero requires
// 16 kHz mono f32 input).
/// Target sample rate handed to whisper-cli. Whisper expects 16 kHz; doing
/// the conversion here with a proper anti-aliased FFT resampler measurably
/// improves quality vs. letting whisper's front-end resample on the fly.
const TARGET_SAMPLE_RATE: u32 = 16_000;
/// Whisper is a mono ASR model; we always downmix.
const TARGET_CHANNELS: u16 = 1;
/// 16-bit PCM is whisper-cli's preferred WAV format and keeps file size
/// trivial (256 kbps ≈ 32 KB/s) — no need for a compressed codec.
const TARGET_BITS_PER_SAMPLE: u16 = 16;
/// Recordings shorter than this after VAD trim are discarded as noise /
/// accidental key-taps. 100 ms ≈ 1600 samples at 16 kHz.
const MIN_RECORDING_MS: u32 = 100;

/// Mic warmth — how long the cpal input stream stays open after a recording
/// ends. Trade-off:
///   - warm  → next press skips CoreAudio cold-start (~200 ms) and pre-roll
///     ring stays primed for leading-word capture;
///   - cold  → macOS immediately restores normal system audio routing
///     (YouTube/music stops sounding like a phone call).
///
/// User-controllable via `settings.audio.idle_timeout_secs`. Read on every
/// `stop()` / `cancel()` so changes take effect on the next press without a
/// restart. `0` means "close stream immediately" — handled inline in
/// `stop()`/`cancel()`, not via the watchdog.
fn idle_timeout_from_settings() -> Duration {
    Duration::from_secs(crate::settings::idle_timeout_secs() as u64)
}

/// How often the idle watchdog wakes up to check whether the warm stream
/// should be closed. 1 s is plenty — IDLE_TIMEOUT is in tens of seconds
/// and the watchdog only does an `Instant` compare and an atomic load
/// when nothing is due.
const WATCHDOG_TICK: Duration = Duration::from_secs(1);

/// Pre-roll ring buffer length, in milliseconds. While the cpal
/// stream is warm but the user hasn't pressed PTT yet, incoming samples
/// are written into a fixed-size ring buffer. On PTT-down, the ring is
/// drained into the per-recording `samples` buffer so the first ~300 ms
/// of audio (including the leading phoneme of a word the user started
/// before the key registered) is captured.
///
/// 300 ms is a deliberate compromise: long enough to catch most leading-
/// word clip-offs (typical word length 150–400 ms), short enough that we
/// don't accidentally include a prior cough or click. Hardcoded; no
/// user-facing setting.
const PREROLL_MS: u32 = 300;

struct ActiveStream {
    _stream: cpal::Stream,
    sample_rate: u32,
    channels: u16,
}

pub struct AudioCapture {
    /// Lock-free SPSC producer written by the cpal callback. The callback
    /// holds a parking_lot Mutex briefly (uncontended — start() is the
    /// only other accessor), but never allocates: the ring is pre-allocated
    /// and push_partial_slice is just a memcpy.
    samples_producer: Arc<Mutex<Option<rtrb::Producer<f32>>>>,
    /// SPSC consumer read by the capture-feeder thread and drained by
    /// stop() for the batch fallback.
    samples_consumer: Arc<Mutex<Option<rtrb::Consumer<f32>>>>,
    /// Feeder accumulates every chunk it reads here; stop() uses this for
    /// the batch path (which needs ALL captured samples, not just what's
    /// left in the ring after the feeder exits).
    samples_accum: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>, // current RMS as f32 bits
    is_recording: Arc<AtomicBool>,
    /// Set true by the cpal callback the first time it appends samples after
    /// `is_recording` flips on. Lets the hotkey path hold the overlay on the
    /// yellow "connecting" tile until CoreAudio actually delivers audio, then
    /// flash red — so the user never starts talking into a cold (silent)
    /// stream. Cleared at the top of every `start()`. Read via `audio_live()`.
    first_sample: Arc<AtomicBool>,
    /// Set by the cpal error callback when CoreAudio reports the device went
    /// away mid-recording (e.g. AirPods disconnected). Read edge-triggered by
    /// `device_lost()` — swap-to-false on read so a single device-loss event
    /// surfaces exactly once.
    device_lost: Arc<AtomicBool>,
    /// Holds the warm cpal stream between recordings. The
    /// stream is opened once per device, gated by the `is_recording`
    /// flag inside the cpal callback, and torn down by the idle
    /// watchdog or by a device-change check at the next `start()`.
    /// Stored behind `Arc` so the watchdog thread can hold its own
    /// handle without borrowing `&self`.
    warm_stream: Arc<Mutex<Option<ActiveStream>>>,
    /// Name of the device the `warm_stream` was opened against. Used at
    /// the next `start()` to decide whether the warm stream still
    /// matches the configured device. Same `Arc<Mutex>` pattern as
    /// `warm_stream` for the same reason.
    warm_device_name: Arc<Mutex<Option<String>>>,
    /// `Some(deadline)` after `stop()` / `cancel()` returns; the
    /// watchdog reads this and closes the warm stream once
    /// `Instant::now() >= deadline`. `None` means "do not close" —
    /// either we're recording right now, or there's no warm stream.
    idle_close_at: Arc<Mutex<Option<Instant>>>,
    /// Idle-watchdog thread handle. Joined on `Drop`.
    watchdog_handle: Mutex<Option<JoinHandle<()>>>,
    /// Set on `Drop` to tell the watchdog to exit.
    shutdown_watchdog: Arc<AtomicBool>,
    /// Streaming finalizer worker (resample + VAD off the
    /// post-release critical path). Spawned in `start()`, shut down in
    /// `stop()` / `cancel()`. The capture-feeder thread (`feeder`) ships
    /// chunks to it from the shared `samples` buffer.
    ///
    /// On streaming degradation (worker init failure, channel disconnect)
    /// `stop()` falls back to the legacy batch finalizer path against
    /// the canonical `samples` buffer — no recording is ever lost.
    streaming: Mutex<Option<StreamingFinalizer>>,
    /// Segment receiver from the streaming finalizer. Stored here so
    /// the hotkey handler can take it via `take_segment_receiver()` right
    /// after `start()` and hand it to `SegmentTranscriber`. Cleared on
    /// `cancel()` and when hotkey takes it; harmlessly empty in `stop()`.
    segment_rx: Mutex<Option<SegReceiver<SegmentEmit>>>,
    /// Capture-feeder thread handle. The feeder polls `samples` and ships
    /// each new chunk to the streaming worker. Joined by `stop()` /
    /// `cancel()` after `feeder_stop` is set so it returns cleanly.
    feeder: Mutex<Option<JoinHandle<()>>>,
    /// Set true to ask the feeder thread to drain pending samples, send
    /// them to the worker, and exit. The feeder polls this every ~10 ms.
    feeder_stop: Arc<AtomicBool>,
    /// Pre-roll ring buffer. Filled by the cpal callback every
    /// tick (regardless of `is_recording`); drained by `start()` on
    /// PTT-down and prepended to `samples`. Holds raw native-rate,
    /// native-channel samples — same format as `samples` mid-recording,
    /// so no per-callback DSP is added. Bounded to `preroll_capacity`
    /// samples (~300 ms at native rate × channels), so memory cost is
    /// fixed and small (~115 KB at 48 kHz stereo f32).
    preroll: Arc<Mutex<VecDeque<f32>>>,
    /// Capacity in samples for the pre-roll ring. Set when the stream is
    /// opened so the cpal callback can read it without taking the
    /// `preroll` lock to size-check. Cleared (set to 0) when the warm
    /// stream is closed so a stale capacity from the previous device's
    /// rate/channels can never affect the next stream's ring.
    preroll_capacity: Arc<AtomicUsize>,
    /// Reusable scratch buffer for i16/u16 sample-format callbacks.
    /// Pre-allocated and cleared (not reallocated) each callback so the
    /// audio thread never heap-allocates. f32 is the common path and
    /// doesn't need conversion, so this field stays cold on most devices.
    callback_scratch: Arc<Mutex<Vec<f32>>>,
}

/// Why a recording was thrown away instead of returning a WAV path.
#[derive(Debug, Clone, Copy)]
pub enum DiscardReason {
    /// Recording was below the minimum length (after silence trim).
    TooShort { duration_ms: u32 },
    /// Whole recording was below the silence threshold.
    Silent,
    /// `stop()` was called with no active stream.
    NoStream,
}

/// Result of a `stop()` call. The `Wav` variant carries a `TempPath` whose
/// `Drop` removes the on-disk WAV — callers don't need to clean up.
pub enum StopOutcome {
    Wav {
        path: TempPath,
        speech_detected: bool,
        /// True when the WAV contains the ENTIRE recording (not just the
        /// tail after the last segment cut). The caller must then ignore
        /// segment transcriptions for final assembly — they are preview-only
        /// — or segment text would be duplicated.
        full_capture: bool,
    },
    Discard(DiscardReason),
}

// SAFETY (Send + Sync for AudioCapture, Send for ActiveStream):
//
// `cpal::Stream` on macOS (cpal 0.15.3) is `Arc<Mutex<StreamInner>>` where
// `StreamInner` contains:
//   1. an `AudioUnit` — coreaudio-rs declares `unsafe impl Send for AudioUnit`
//      (coreaudio-rs 0.11.3 audio_unit/mod.rs:317). `AudioUnit::drop` calls
//      `AudioOutputUnitStop`, `AudioUnitUninitialize`, and
//      `AudioComponentInstanceDispose`, all documented by Apple as callable
//      from any thread. So the AudioUnit half is naturally Send.
//   2. a `_disconnect_listener: Option<AudioObjectPropertyListener>` carrying
//      a `Box<dyn FnMut()>` (no `+ Send` bound). This is the *only* reason
//      cpal::Stream is `!Send` on macOS — a missing bound, not a real
//      threading hazard. The closure captures a clone of the Stream and an
//      `Arc<Mutex<E>>` error callback; both are Send when `E: Send`, which
//      our `err_fn` is. cpal also only attaches this listener when the
//      device is non-default (cpal macos/mod.rs:614-618). For our default-
//      device path, `_disconnect_listener` is `None` and Stream is morally
//      Send already.
//
// Concrete access pattern in this codebase, threading the needle:
//   - Hotkey thread (the CGEventTap callback in hotkey.rs runs on a single
//     dedicated OS thread that owns the CFRunLoop): the primary thread
//     that touches `self.warm_stream`. It calls `start()` (creating or
//     reusing the Stream) and `stop()` (leaving the Stream warm). The
//     `warm_stream: Mutex<Option<ActiveStream>>` is the only path to
//     read or replace the stream; it serializes against any pathological
//     re-entry.
//   - Idle-watchdog thread (spawned in `AudioCapture::new`): also touches
//     `self.warm_stream`, but only to drop a stale warm stream after
//     `IDLE_TIMEOUT` has elapsed. The mutex serializes with the hotkey
//     thread; the `is_recording` flag check inside the watchdog covers
//     the race where the hotkey thread flips back to recording between
//     the watchdog reading `idle_close_at` and acquiring the mutex.
//   - Level-broadcast thread (spawned in lib.rs): calls `level()` which
//     reads the `level: Arc<AtomicU32>` only, and `recorder.is_recording()`
//     which reads Recorder's own `Mutex<State>` — never touches
//     `warm_stream` and therefore never touches the cpal::Stream.
//   - CoreAudio callback thread (cpal-managed): the callback closure
//     captures clones of `is_recording` (AtomicBool), `samples_producer`
//     (Arc<Mutex<Option<Producer>>>) and `level` (AtomicU32) — never the
//     `warm_stream` field, never the Stream itself, never `&self`.
//
// So `warm_stream` is touched from at most two threads (hotkey + watchdog)
// always behind a parking_lot Mutex, and every other field is already-
// Send-and-Sync (atomics + `Arc<Mutex<Vec<f32>>>`). No data race on the
// Stream is reachable. The unsafe impls patch over a missing Send bound
// on a `Box<dyn FnMut()>` deep inside cpal; they do not paper over any
// real concurrent access.
unsafe impl Send for AudioCapture {}
unsafe impl Sync for AudioCapture {}
unsafe impl Send for ActiveStream {}

fn rms(data: &[f32]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    (data.iter().map(|&s| s * s).sum::<f32>() / data.len() as f32).sqrt()
}

/// Average interleaved multi-channel frames down to mono. cpal hands us samples
/// as `[L0, R0, L1, R1, ...]` for stereo; whisper wants a single channel.
fn downmix_to_mono(buf: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return buf.to_vec();
    }
    let ch = channels as usize;
    let frames = buf.len() / ch;
    let inv = 1.0 / ch as f32;
    let mut out = Vec::with_capacity(frames);
    for i in 0..frames {
        let start = i * ch;
        let mut acc = 0.0_f32;
        for c in 0..ch {
            acc += buf[start + c];
        }
        out.push(acc * inv);
    }
    out
}

/// Resample mono `f32` PCM at `src_rate` to 16 kHz using `rubato::FftFixedIn`.
/// Whisper expects 16 kHz; doing the conversion ourselves with proper
/// anti-aliasing (FFT-based) measurably improves quality vs. letting whisper's
/// front-end resample on the fly, especially from 44.1/48 kHz Bluetooth devices.
///
/// `rubato` is a fixed-chunk-in resampler, so we feed `RESAMPLER_CHUNK_SIZE`-
/// sample chunks and pad the trailing remainder with zeros. The FFT pipeline
/// adds a small amount of latency-padding to the output; we estimate the ideal
/// output length from the rate ratio and truncate to that — this keeps the
/// downstream VAD frame indexing honest about timing.
fn resample_to_16k(buf: &[f32], src_rate: u32) -> anyhow::Result<Vec<f32>> {
    use rubato::{FftFixedIn, Resampler};

    const TARGET_RATE: usize = TARGET_SAMPLE_RATE as usize;
    const CHUNK_IN: usize = 1024;

    if src_rate as usize == TARGET_RATE {
        return Ok(buf.to_vec());
    }
    if buf.is_empty() {
        return Ok(Vec::new());
    }

    let mut resampler = FftFixedIn::<f32>::new(
        src_rate as usize,
        TARGET_RATE,
        CHUNK_IN,
        /* sub_chunks  */ 2,
        /* nbr_channels*/ 1,
    )?;

    // FftFixedIn carries a fixed `output_delay` of zero-padding at the start of
    // the stream — drop it so output[0] corresponds to input[0]. We also need
    // to feed extra zero-padding past the end of the real input so the filter
    // tail flushes the last `output_delay` samples of useful output.
    let delay = resampler.output_delay();

    let mut out: Vec<f32> = Vec::with_capacity(
        (buf.len() as u64 * TARGET_RATE as u64 / src_rate as u64) as usize + CHUNK_IN,
    );

    let ideal_len = (buf.len() as u64 * TARGET_RATE as u64 / src_rate as u64) as usize;
    let target_total = ideal_len + delay; // pre-skip
    let mut i = 0;
    while out.len() < target_total {
        let mut chunk_buf: [f32; CHUNK_IN] = [0.0; CHUNK_IN];
        let take = CHUNK_IN.min(buf.len().saturating_sub(i));
        if take > 0 {
            chunk_buf[..take].copy_from_slice(&buf[i..i + take]);
            i += take;
        }
        let processed = resampler.process(&[&chunk_buf[..]], None)?;
        out.extend_from_slice(&processed[0]);
    }

    // Drop the resampler's leading delay so output is time-aligned to input,
    // then truncate to the ideal output length.
    if delay > 0 && out.len() > delay {
        out.drain(..delay);
    }
    if out.len() > ideal_len {
        out.truncate(ideal_len);
    }
    Ok(out)
}

/// Normalizes below-peak audio to the target level without attenuating.
/// MacBook microphones peak between -25 and -18 dBFS; boosting before
/// transcription reduces hallucinations on quiet audio.
fn peak_normalize(samples: &mut [f32], target: f32) {
    let peak = samples.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
    if peak > 0.0 && peak < target {
        let gain = target / peak;
        for s in samples.iter_mut() {
            *s = (*s * gain).clamp(-1.0, 1.0);
        }
    }
}

/// Build the `hound::WavSpec` for the on-disk handoff to whisper-cli. Pulled
/// out so a unit test can pin the exact contract (16 kHz mono 16-bit PCM int)
/// and so `stop()` doesn't grow another magic-number block.
fn whisper_wav_spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: TARGET_CHANNELS,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: TARGET_BITS_PER_SAMPLE,
        sample_format: hound::SampleFormat::Int,
    }
}

/// On-disk WAV contract for all transcription backends: 16 kHz mono 16-bit PCM
/// int. Matches `AudioCapture::write_wav` and `transcribe-rs` `read_wav_samples`.
pub(crate) fn write_transcription_wav(
    path: &std::path::Path,
    samples: &[f32],
) -> anyhow::Result<()> {
    let mut writer = hound::WavWriter::create(path, whisper_wav_spec())?;
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(v)?;
    }
    writer.finalize()?;
    Ok(())
}

impl AudioCapture {
    pub fn new() -> anyhow::Result<Self> {
        let capture = Self {
            samples_producer: Arc::new(Mutex::new(None)),
            samples_consumer: Arc::new(Mutex::new(None)),
            samples_accum: Arc::new(Mutex::new(Vec::new())),
            level: Arc::new(AtomicU32::new(0)),
            is_recording: Arc::new(AtomicBool::new(false)),
            first_sample: Arc::new(AtomicBool::new(false)),
            device_lost: Arc::new(AtomicBool::new(false)),
            warm_stream: Arc::new(Mutex::new(None)),
            warm_device_name: Arc::new(Mutex::new(None)),
            idle_close_at: Arc::new(Mutex::new(None)),
            watchdog_handle: Mutex::new(None),
            shutdown_watchdog: Arc::new(AtomicBool::new(false)),
            streaming: Mutex::new(None),
            segment_rx: Mutex::new(None),
            feeder: Mutex::new(None),
            feeder_stop: Arc::new(AtomicBool::new(false)),
            preroll: Arc::new(Mutex::new(VecDeque::new())),
            preroll_capacity: Arc::new(AtomicUsize::new(0)),
            callback_scratch: Arc::new(Mutex::new(Vec::with_capacity(4096))),
        };
        capture.spawn_watchdog();
        Ok(capture)
    }

    /// Spawn the idle watchdog. Loops until `shutdown_watchdog` is set;
    /// every `WATCHDOG_TICK` it checks whether the warm stream is past
    /// its idle deadline and, if so, closes it. The `is_recording`
    /// check inside guards against a race where `start()` flipped the
    /// flag between the watchdog reading `idle_close_at` and acquiring
    /// the `warm_stream` mutex — see the SAFETY block above for the
    /// thread-access pattern.
    fn spawn_watchdog(&self) {
        let shutdown = self.shutdown_watchdog.clone();
        let idle_close_at = self.idle_close_at.clone();
        let is_recording = self.is_recording.clone();
        let warm_stream = self.warm_stream.clone();
        let warm_device_name = self.warm_device_name.clone();
        let preroll = self.preroll.clone();
        let preroll_capacity = self.preroll_capacity.clone();

        let handle = std::thread::Builder::new()
            .name("turbotalk-audio-watchdog".into())
            .spawn(move || loop {
                std::thread::sleep(WATCHDOG_TICK);
                if shutdown.load(Ordering::SeqCst) {
                    return;
                }
                let due = {
                    let guard = idle_close_at.lock();
                    match *guard {
                        Some(deadline) => Instant::now() >= deadline,
                        None => false,
                    }
                };
                if !due {
                    continue;
                }
                // Race-safe close: re-check is_recording under the
                // warm_stream lock. If start() flipped the flag while
                // we were waking up, leave the warm stream alone.
                let mut stream_guard = warm_stream.lock();
                if is_recording.load(Ordering::SeqCst) {
                    // Recording resumed; clear the deadline and keep
                    // the warm stream open.
                    *idle_close_at.lock() = None;
                    continue;
                }
                if stream_guard.is_some() {
                    *stream_guard = None;
                    *warm_device_name.lock() = None;
                    // Clear stale pre-roll so the next stream
                    // (potentially at a different rate / channels)
                    // starts with an empty ring.
                    preroll.lock().clear();
                    preroll_capacity.store(0, Ordering::SeqCst);
                    tracing::info!("[audio] stream closed (idle timeout)");
                }
                *idle_close_at.lock() = None;
            })
            .expect("spawn audio watchdog thread");
        *self.watchdog_handle.lock() = Some(handle);
    }

    /// Platform-aware microphone permission help text.
    fn mic_permission_help_text() -> &'static str {
        #[cfg(target_os = "macos")]
        {
            "grant permission in System Settings → Privacy → Microphone, then relaunch."
        }
        #[cfg(target_os = "windows")]
        {
            "grant permission in Windows Settings → Privacy & Security → Microphone, then relaunch."
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            "grant microphone permission in your system settings, then relaunch."
        }
    }

    fn open_stream(&self, want: &str) -> anyhow::Result<ActiveStream> {
        let host = cpal::default_host();
        let mic_help = Self::mic_permission_help_text();
        let device = if want == "default" || want.is_empty() {
            host.default_input_device()
                .ok_or_else(|| anyhow::anyhow!("Microphone access denied — {mic_help}"))?
        } else {
            host.input_devices()?
                .find(|d| d.name().ok().as_deref() == Some(want))
                .or_else(|| host.default_input_device())
                .ok_or_else(|| anyhow::anyhow!("Microphone access denied — {mic_help}"))?
        };

        let name = device.name().unwrap_or_else(|_| "unknown".into());
        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let sample_format = config.sample_format();

        tracing::info!(
            "[audio] opening stream: \"{}\" {} Hz {} ch {:?}",
            name,
            sample_rate,
            channels,
            sample_format
        );

        let rec = self.is_recording.clone();
        let lvl = self.level.clone();
        let first = self.first_sample.clone();

        // Create the lock-free SPSC ring buffer for samples. The producer
        // is shared via Arc<Mutex<Option<Producer>>> so that start() can
        // push preroll data through it. The consumer is read by the feeder
        // and stop().
        let (samples_producer, samples_consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
        *self.samples_producer.lock() = Some(samples_producer);
        *self.samples_consumer.lock() = Some(samples_consumer);
        // Clear the feeder's accumulation Vec from the previous life.
        self.samples_accum.lock().clear();
        let smp_producer = self.samples_producer.clone();

        // Size and prepare the pre-roll ring for this stream's
        // native rate / channels. Cleared (not just resized) because a
        // device change between presses must not leak old-device samples
        // into the new ring.
        let preroll_cap = (PREROLL_MS as usize * sample_rate as usize * channels as usize) / 1000;
        self.preroll_capacity.store(preroll_cap, Ordering::SeqCst);
        {
            let mut ring = self.preroll.lock();
            ring.clear();
            ring.reserve(preroll_cap);
        }
        let pre = self.preroll.clone();
        let pre_cap = self.preroll_capacity.clone();

        // Error callback runs on cpal's audio thread. We *cannot* safely call
        // into Tauri (no AppHandle here) — so we set an atomic flag and let
        // the level-broadcast thread (which already polls every 50 ms and has
        // the AppHandle) surface the event to the frontend.
        let dev_lost = self.device_lost.clone();
        let rec_for_err = self.is_recording.clone();
        let err_fn = move |e: cpal::StreamError| {
            match &e {
                cpal::StreamError::DeviceNotAvailable => {
                    tracing::warn!(
                        "[audio] device became unavailable mid-stream — flagging device-lost"
                    );
                    dev_lost.store(true, Ordering::SeqCst);
                    // Stop accumulating samples. The active stream will be
                    // dropped by the broadcast thread when it observes the
                    // flag and calls `recorder.cancel()`.
                    rec_for_err.store(false, Ordering::SeqCst);
                }
                cpal::StreamError::BackendSpecific { err } => {
                    tracing::error!("[audio] backend stream error: {}", err);
                }
            }
        };

        // ---- cpal callback discipline -----------------------------------
        // The audio callback runs on CoreAudio's high-priority thread and
        // must do only:
        //   1. push native-rate samples into the lock-free SPSC ring
        //      (`producer.push_partial_slice` — no allocation, no realloc);
        //   2. update the level meter atomic.
        //
        // The ring is sized at RING_CAPACITY (~5 s at 48 kHz stereo f32)
        // and is pre-allocated at stream-open time. No heap-alloc occurs
        // in the callback. The streaming finalizer pulls samples via the
        // capture-feeder thread, never here.
        //
        // Feed the pre-roll ring on every callback regardless
        // of `is_recording`. Operations: lock → extend → trim front if
        // over capacity → unlock. One short critical section, called
        // per CoreAudio buffer (~10 ms). If profiling shows contention,
        // swap for an SPSC ring (e.g. `rtrb`) — don't pre-optimize.
        fn push_preroll(ring: &Mutex<VecDeque<f32>>, capacity: usize, data: &[f32]) {
            if capacity == 0 {
                return;
            }
            let mut g = ring.lock();
            g.extend(data.iter().copied());
            if g.len() > capacity {
                let drop = g.len() - capacity;
                g.drain(0..drop);
            }
        }

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &_| {
                        // CALLBACK-ALLOWED-OPS: pre-roll push, ring push, level.
                        let cap = pre_cap.load(Ordering::Relaxed);
                        push_preroll(&pre, cap, data);
                        if rec.load(Ordering::Relaxed) {
                            // Lock-free push into the pre-allocated ring —
                            // no allocation, no realloc, no lock contention.
                            if let Some(p) = smp_producer.lock().as_mut() {
                                let _ = p.push_partial_slice(data);
                            }
                            first.store(true, Ordering::Relaxed);
                            lvl.store(rms(data).to_bits(), Ordering::Relaxed);
                        } else {
                            lvl.store(0_f32.to_bits(), Ordering::Relaxed);
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            cpal::SampleFormat::I16 => {
                let scratch = self.callback_scratch.clone();
                let smp_i16 = smp_producer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &_| {
                        let mut floats = scratch.lock();
                        floats.clear();
                        floats.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                        let cap = pre_cap.load(Ordering::Relaxed);
                        push_preroll(&pre, cap, &floats);
                        if rec.load(Ordering::Relaxed) {
                            if let Some(p) = smp_i16.lock().as_mut() {
                                let _ = p.push_partial_slice(&floats);
                            }
                            first.store(true, Ordering::Relaxed);
                            lvl.store(rms(&floats).to_bits(), Ordering::Relaxed);
                        } else {
                            lvl.store(0_f32.to_bits(), Ordering::Relaxed);
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            cpal::SampleFormat::U16 => {
                let scratch = self.callback_scratch.clone();
                let smp_u16 = smp_producer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[u16], _: &_| {
                        let mut floats = scratch.lock();
                        floats.clear();
                        floats.extend(data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0));
                        let cap = pre_cap.load(Ordering::Relaxed);
                        push_preroll(&pre, cap, &floats);
                        if rec.load(Ordering::Relaxed) {
                            if let Some(p) = smp_u16.lock().as_mut() {
                                let _ = p.push_partial_slice(&floats);
                            }
                            first.store(true, Ordering::Relaxed);
                            lvl.store(rms(&floats).to_bits(), Ordering::Relaxed);
                        } else {
                            lvl.store(0_f32.to_bits(), Ordering::Relaxed);
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            other => anyhow::bail!("unsupported sample format: {:?}", other),
        };

        stream.play()?;
        Ok(ActiveStream {
            _stream: stream,
            sample_rate,
            channels,
        })
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// True once the cpal callback has appended at least one buffer of
    /// post-press audio since the last `start()`. On a cold start this lags
    /// `start()` by the CoreAudio cold-start latency (~200 ms, more with a
    /// route switch); on a warm stream it flips within one callback (~10 ms).
    /// The hotkey path polls this to gate the red "recording" indicator on
    /// real capture instead of `stream.play()` returning.
    pub fn audio_live(&self) -> bool {
        self.first_sample.load(Ordering::Relaxed)
    }

    /// Edge-triggered read: returns true once when the cpal error callback
    /// flagged the device as gone, then resets so the next caller sees false.
    pub fn device_lost(&self) -> bool {
        self.device_lost.swap(false, Ordering::SeqCst)
    }

    /// Take the segment receiver produced by the most recent `start()`.
    /// Returns `None` if no recording is in progress or the receiver was
    /// already taken. The caller (hotkey.rs) passes this to
    /// `SegmentTranscriber::start` so segments are transcribed concurrently
    /// with recording rather than all at once after release.
    pub fn take_segment_receiver(&self) -> Option<SegReceiver<SegmentEmit>> {
        self.segment_rx.lock().take()
    }

    pub fn start(&self) -> anyhow::Result<()> {
        // Discard any stale data still in the samples ring from the previous
        // recording. The feeder reads from the consumer; we need it empty so
        // only new callback data (plus preroll below) enters this session.
        {
            let mut cg = self.samples_consumer.lock();
            if let Some(c) = cg.as_mut() {
                while c.pop().is_ok() {}
            }
        }
        self.samples_accum.lock().clear();
        self.level.store(0_f32.to_bits(), Ordering::Relaxed);
        // Re-arm the first-sample gate. The cpal callback sets it true once it
        // appends post-press audio; the hotkey path polls `audio_live()` to
        // know when CoreAudio is genuinely delivering before flipping the
        // overlay from yellow "connecting" to red "recording".
        self.first_sample.store(false, Ordering::SeqCst);
        // If the previous session ended with device-lost, the warm
        // stream is broken — drop it so we re-open below.
        let device_was_lost = self.device_lost.swap(false, Ordering::SeqCst);
        if device_was_lost {
            *self.warm_stream.lock() = None;
            *self.warm_device_name.lock() = None;
            // Ring was sized for the (now-broken) old stream.
            // Clear it; open_stream() will re-size on the next call.
            self.preroll.lock().clear();
            self.preroll_capacity.store(0, Ordering::SeqCst);
        }
        self.feeder_stop.store(false, Ordering::SeqCst);

        // Always read device from config so changes take effect without restart.
        let want = crate::settings::audio_device();

        // Decide whether to reuse the warm stream or open a fresh one.
        // Holding `warm_stream` across `open_stream` is fine — the only
        // other thread that touches it (the watchdog) just spins on the
        // mutex for at most a few ms.
        let (src_rate, src_channels) = {
            let mut warm = self.warm_stream.lock();
            let mut warm_name = self.warm_device_name.lock();
            let reuse = match (warm.as_ref(), warm_name.as_ref()) {
                (Some(_), Some(name)) => name.as_str() == want.as_str(),
                _ => false,
            };
            if reuse {
                let active = warm.as_ref().expect("reuse implies Some");
                // Resume the paused stream so the callback fires again.
                active._stream.play()?;
                tracing::info!(
                    "[audio] stream reused (warm, resumed) — device=\"{}\" {} Hz {} ch",
                    want,
                    active.sample_rate,
                    active.channels
                );
                (active.sample_rate, active.channels)
            } else {
                if warm.is_some() {
                    *warm = None;
                    *warm_name = None;
                    // Drop ring contents from the old device —
                    // they may be at a different rate / channel layout
                    // than the new stream. open_stream() below resizes
                    // the ring for the new device.
                    self.preroll.lock().clear();
                    self.preroll_capacity.store(0, Ordering::SeqCst);
                    tracing::info!("[audio] stream closed (device change)");
                }
                let stream = self.open_stream(&want)?;
                let rate = stream.sample_rate;
                let channels = stream.channels;
                *warm = Some(stream);
                *warm_name = Some(want.clone());
                tracing::info!("[audio] stream pre-warmed");
                (rate, channels)
            }
        };

        // Cancel any pending idle close — we're about to record.
        *self.idle_close_at.lock() = None;

        // Drain the pre-roll ring into the samples ring so the first
        // ~PREROLL_MS of audio (captured while the stream was warm but
        // is_recording=false) is prepended to the recording.
        // We hold the producer lock briefly to push preroll data; the
        // callback is paused (stream.play() was just called and the
        // first callback hasn't fired yet), so no contention.
        let preroll: Vec<f32> = {
            let mut ring = self.preroll.lock();
            ring.drain(..).collect()
        };
        if !preroll.is_empty() {
            let preroll_ms =
                (preroll.len() as u64 * 1000 / (src_rate as u64 * src_channels as u64)) as u32;
            let n = preroll.len();
            if let Some(p) = self.samples_producer.lock().as_mut() {
                let _ = p.push_partial_slice(&preroll);
            }
            tracing::info!(
                "[audio] start: prepended {} samples of pre-roll ({} ms)",
                n,
                preroll_ms
            );
        } else {
            tracing::info!("[audio] start: pre-roll ring empty (cold start)");
        }

        // Flip recording before the feeder starts so callbacks append
        // post-press samples. Preroll data was already pushed to the ring
        // above; the feeder will read it first.
        self.is_recording.store(true, Ordering::SeqCst);

        // Spawn the streaming finalizer worker and the
        // capture-feeder thread. The worker owns the resampler + VAD
        // state and consumes chunks off the cpal callback's critical
        // path. The feeder polls the consumer every ~10 ms and ships
        // any newly-available samples to the worker.
        let (finalizer, seg_rx) = StreamingFinalizer::start(src_rate, src_channels, NORMALIZE_PEAK);
        *self.streaming.lock() = Some(finalizer);
        *self.segment_rx.lock() = Some(seg_rx);

        let feeder_handle = self.spawn_feeder();
        *self.feeder.lock() = Some(feeder_handle);

        tracing::info!("[audio] recording started");
        Ok(())
    }

    /// Spawn the capture-feeder thread. It polls the lock-free SPSC
    /// consumer for new samples and ships each chunk to the streaming
    /// finalizer via the bounded crossbeam channel. The feeder also
    /// accumulates every chunk into `samples_accum` so that stop()'s
    /// batch fallback holds the complete recording even after the ring
    /// buffer has been consumed.
    fn spawn_feeder(&self) -> JoinHandle<()> {
        let samples_consumer = self.samples_consumer.clone();
        let samples_accum = self.samples_accum.clone();
        let stop_flag = self.feeder_stop.clone();

        let finalizer_sender = self
            .streaming
            .lock()
            .as_ref()
            .map(|f| f.handle())
            .expect("finalizer must exist by start() time");

        std::thread::Builder::new()
            .name("turbotalk-capture-feeder".into())
            .spawn(move || {
                let poll_interval = Duration::from_millis(10);
                let mut backpressure_drops: u64 = 0;

                loop {
                    let stop = stop_flag.load(Ordering::SeqCst);

                    // Read available samples from the ring buffer consumer.
                    // Hold the lock only across the copy-out; never across
                    // the channel send or the accum Vec.
                    let chunk: Option<Vec<f32>> = {
                        let mut cg = samples_consumer.lock();
                        let consumer = match cg.as_mut() {
                            Some(c) => c,
                            None => return,
                        };
                        let n = consumer.slots();
                        if n > 0 {
                            let mut buf = vec![0.0f32; n];
                            let (filled, _) = consumer.pop_partial_slice(&mut buf);
                            // filled spans the whole buf (we allocated n
                            // which is exactly what slots() reported).
                            let _ = filled;
                            Some(buf)
                        } else {
                            None
                        }
                    };

                    if let Some(data) = chunk {
                        // Accumulate for the batch fallback.
                        samples_accum.lock().extend_from_slice(&data);

                        match finalizer_sender.try_send(data) {
                            Ok(()) => {}
                            Err(DropReason::WorkerBackpressure) => {
                                // Drop on the capture-feeder side, never
                                // on the cpal callback side. The
                                // canonical recording is in `samples_accum`,
                                // so stop()'s batch fallback still has
                                // everything.
                                backpressure_drops += 1;
                            }
                            Err(DropReason::WorkerGone) => {
                                tracing::warn!(
                                    "[audio] streaming finalizer worker disconnected — \
                                     feeder exiting; stop() will use batch fallback"
                                );
                                break;
                            }
                        }
                    }

                    if stop {
                        if backpressure_drops > 0 {
                            tracing::info!(
                                "[audio] capture-feeder exiting; {} chunks dropped on \
                                 worker backpressure (batch fallback covers these)",
                                backpressure_drops
                            );
                        }
                        return;
                    }

                    std::thread::sleep(poll_interval);
                }
            })
            .expect("spawn capture-feeder thread")
    }

    /// Arm the warm-stream close path according to the user's mic-warmth
    /// setting. `0` (OFF) closes the stream synchronously here; any non-zero
    /// value defers to the idle watchdog by setting `idle_close_at` to
    /// `now + N seconds`. Pulled out so `stop()` and `cancel()` agree
    /// bit-for-bit on the warm-vs-cold decision.
    ///
    /// When keeping the stream warm we also pause it so the cpal callback
    /// stops firing; `start()` calls `play()` to resume it.
    fn arm_or_close_warm_stream(&self) {
        let timeout = idle_timeout_from_settings();
        if timeout.is_zero() {
            // OFF: drop the cpal stream immediately so macOS releases the
            // input session and system audio routing returns to normal.
            // This also clears the pre-roll ring; the next press pays
            // the cold-start latency on purpose.
            *self.warm_stream.lock() = None;
            *self.warm_device_name.lock() = None;
            self.preroll.lock().clear();
            self.preroll_capacity.store(0, Ordering::SeqCst);
            *self.idle_close_at.lock() = None;
            tracing::info!("[audio] stream closed (mic warmth OFF)");
        } else {
            // Pause the stream so the callback stops firing during idle.
            // The callback's is_recording gate still keeps samples out of
            // the recording, but pausing the stream itself means CoreAudio
            // doesn't even invoke the callback — no wakeups, no mutex, no
            // preroll accumulation.
            if let Some(active) = self.warm_stream.lock().as_ref() {
                let _ = active._stream.pause();
            }
            *self.idle_close_at.lock() = Some(Instant::now() + timeout);
        }
    }

    /// Cancel the in-flight recording. Tears down the per-recording
    /// streaming worker + feeder and clears the sample buffer, but
    /// leaves the cpal stream warm for the next press — unless the
    /// cancellation is itself the response to a device-lost event, in
    /// which case the stream is broken and we drop it. The idle
    /// watchdog will close the warm stream after the configured mic
    /// warmth (`settings.audio.idle_timeout_secs`).
    pub fn cancel(&self) {
        self.cancel_inner(false);
    }

    pub fn cancel_after_device_lost(&self) {
        self.cancel_inner(true);
    }

    fn cancel_inner(&self, device_lost: bool) {
        self.is_recording.store(false, Ordering::SeqCst);

        // Tear down the streaming finalizer + feeder. We drop the
        // finalizer (which closes the channel and joins the worker)
        // and ask the feeder to exit; the worker discards whatever it
        // had in flight — there's no result to deliver.
        self.feeder_stop.store(true, Ordering::SeqCst);
        let feeder = self.feeder.lock().take();
        if let Some(h) = feeder {
            let _ = h.join();
        }
        let streaming = self.streaming.lock().take();
        drop(streaming);
        // Drop any un-taken segment receiver — no segments to transcribe on cancel.
        *self.segment_rx.lock() = None;

        // Device-lost: the underlying cpal stream is broken — drop it
        // immediately so the next start() opens a fresh stream against
        // whatever device is now configured. We read `device_lost`
        // directly (without the swap-on-read accessor): the level
        // thread already swapped the flag when it observed the loss
        // and called cancel(). At this point start() will see
        // `device_lost=false` again, but the broken stream is already
        // gone here.
        if device_lost || self.device_lost.load(Ordering::SeqCst) {
            *self.warm_stream.lock() = None;
            *self.warm_device_name.lock() = None;
            self.preroll.lock().clear();
            self.preroll_capacity.store(0, Ordering::SeqCst);
            tracing::info!("[audio] stream closed (device lost)");
        }

        // Discard any remaining samples in the ring and the accumulation.
        {
            let mut cg = self.samples_consumer.lock();
            if let Some(c) = cg.as_mut() {
                while c.pop().is_ok() {}
            }
        }
        self.samples_accum.lock().clear();
        self.level.store(0_f32.to_bits(), Ordering::Relaxed);

        // Honour the mic-warmth setting: either arm the watchdog or close
        // the warm stream right now (if `idle_timeout_secs == 0`).
        self.arm_or_close_warm_stream();

        tracing::info!("[audio] recording cancelled");
    }

    pub fn stop(&self) -> anyhow::Result<StopOutcome> {
        // Stage timing — see ARCHITECTURE.md "Audio Pipeline Contract".
        // We log a single compact line at the end of finalization so later
        // optimization work is grounded in measurements instead of vibes.
        // The legacy `total=` field is preserved for continuity, with
        // additional `incremental_resample_total`, `incremental_vad_total`,
        // and `finalize_flush` fields when the streaming path is used.
        let t_total_start = Instant::now();

        self.is_recording.store(false, Ordering::SeqCst);
        // Let the last in-flight callback finish (CoreAudio buffer ≈ 10ms).
        // Reduced from 25 ms — one full CoreAudio buffer cycle is sufficient margin.
        std::thread::sleep(Duration::from_millis(10));

        // Read sample-rate / channels from the warm stream. The mic-warmth
        // block below decides whether to leave the stream warm (watchdog
        // closes it after `idle_timeout_secs`) or drop it right now (OFF).
        let (src_sample_rate, src_channels) = {
            let warm = self.warm_stream.lock();
            match warm.as_ref() {
                Some(a) => (a.sample_rate, a.channels),
                None => return Ok(StopOutcome::Discard(DiscardReason::NoStream)),
            }
        };

        // Honour the mic-warmth setting. We've already snapshotted the
        // stream's sample-rate / channels above, so it's safe to drop the
        // warm stream right now if the user chose OFF — macOS will release
        // the input session and system audio routing returns to normal
        // before transcription even runs.
        self.arm_or_close_warm_stream();

        // Signal the capture-feeder to drain remaining samples and
        // exit, then join it. After this the streaming worker has
        // received every sample the cpal callback ever wrote.
        self.feeder_stop.store(true, Ordering::SeqCst);
        let feeder = self.feeder.lock().take();
        if let Some(h) = feeder {
            let _ = h.join();
        }

        // Try the streaming finalizer first.
        let mut capture_clone_ms = 0.0f32;
        let streaming = self.streaming.lock().take();
        if let Some(finalizer) = streaming {
            let t_streaming_finish = Instant::now();
            if let Some(result) = finalizer.finish() {
                let streaming_finish_ms = t_streaming_finish.elapsed().as_secs_f32() * 1000.0;

                if result.resampled_total > 0 {
                    return self.write_wav_from_streaming_result(
                        result,
                        capture_clone_ms,
                        streaming_finish_ms,
                        t_total_start,
                    );
                }

                // Worker degraded (resampler init failure or zero
                // throughput). Fall through to the batch path below
                // against `buf_full` — no recording is lost.
                tracing::warn!(
                    "[audio] streaming finalizer produced no output — \
                     falling back to batch finalizer"
                );
            } else {
                tracing::warn!(
                    "[audio] streaming finalizer worker did not deliver a result — \
                     falling back to batch finalizer"
                );
            }
        }

        // ---- Batch fallback path -------------------------------------
        // Reached only when the streaming path is degraded (worker init
        // failure, mid-stream VAD/resampler error). We reconstruct the
        // full recording from two sources:
        //   1. `samples_accum` — everything the feeder read from the ring
        //      and shipped to the (now-dead) streaming worker.
        //   2. `samples_consumer` — whatever the feeder hadn't yet read
        //      from the ring before it exited (typically the last
        //      10–20 ms of audio).
        let t_capture_clone_start = Instant::now();

        // Drain the feeder's accumulation (fast path — just clone the Vec).
        let accum = self.samples_accum.lock().clone();

        // Drain any remaining data from the ring consumer (data the feeder
        // hadn't polled yet).
        let remaining: Vec<f32> = {
            let mut cg = self.samples_consumer.lock();
            match cg.as_mut() {
                Some(c) => {
                    let n = c.slots();
                    if n > 0 {
                        let mut buf = vec![0.0f32; n];
                        let (filled, _) = c.pop_partial_slice(&mut buf);
                        let _ = filled;
                        buf
                    } else {
                        Vec::new()
                    }
                }
                None => Vec::new(),
            }
        };

        let mut buf_full = accum;
        buf_full.extend(remaining);
        capture_clone_ms = t_capture_clone_start.elapsed().as_secs_f32() * 1000.0;

        tracing::info!(
            "[audio] {} samples captured ({} Hz, {} ch — pre-resample, batch fallback)",
            buf_full.len(),
            src_sample_rate,
            src_channels
        );

        let t_downmix_start = Instant::now();
        let buf = downmix_to_mono(&buf_full, src_channels);
        let downmix_ms = t_downmix_start.elapsed().as_secs_f32() * 1000.0;

        let t_resample_start = Instant::now();
        let buf = resample_to_16k(&buf, src_sample_rate)?;
        let resample_ms = t_resample_start.elapsed().as_secs_f32() * 1000.0;

        tracing::info!(
            "[audio] {} samples after resample ({} Hz, {} ch — batch fallback)",
            buf.len(),
            TARGET_SAMPLE_RATE,
            TARGET_CHANNELS
        );

        let t_vad_start = Instant::now();
        let (start, end) = crate::vad::trim(&buf);
        let mut trimmed: Vec<f32> = buf[start..end].to_vec();
        let vad_ms = t_vad_start.elapsed().as_secs_f32() * 1000.0;

        let min_samples = (TARGET_SAMPLE_RATE as u64 * MIN_RECORDING_MS as u64 / 1000) as usize;
        if trimmed.len() < min_samples {
            let duration_ms = (trimmed.len() as u64 * 1000 / TARGET_SAMPLE_RATE as u64) as u32;
            let total_ms = t_total_start.elapsed().as_secs_f32() * 1000.0;
            tracing::info!(
                "[audio] recording too short after trim ({} samples, {} ms) — skipping",
                trimmed.len(),
                duration_ms
            );
            tracing::info!(
                "[audio] stage timings (ms): capture_clone={:.2} downmix={:.2} resample={:.2} vad={:.2} normalize=0.00 wav_write=0.00 total={:.2} (discarded: too_short, batch_fallback)",
                capture_clone_ms, downmix_ms, resample_ms, vad_ms, total_ms
            );
            return Ok(StopOutcome::Discard(DiscardReason::TooShort {
                duration_ms,
            }));
        }

        let t_normalize_start = Instant::now();
        peak_normalize(&mut trimmed, NORMALIZE_PEAK);
        let normalize_ms = t_normalize_start.elapsed().as_secs_f32() * 1000.0;

        let t_wav_start = Instant::now();
        let temp_path = self.write_wav(&trimmed)?;
        let wav_write_ms = t_wav_start.elapsed().as_secs_f32() * 1000.0;

        let total_ms = t_total_start.elapsed().as_secs_f32() * 1000.0;
        tracing::info!(
            "[audio] wrote {} samples ({:.2}s trimmed) → {:?} (batch_fallback)",
            trimmed.len(),
            trimmed.len() as f32 / TARGET_SAMPLE_RATE as f32,
            temp_path.to_path_buf(),
        );
        tracing::info!(
            "[audio] stage timings (ms): capture_clone={:.2} downmix={:.2} resample={:.2} vad={:.2} normalize={:.2} wav_write={:.2} total={:.2} (batch_fallback)",
            capture_clone_ms, downmix_ms, resample_ms, vad_ms, normalize_ms, wav_write_ms, total_ms
        );
        Ok(StopOutcome::Wav {
            path: temp_path,
            speech_detected: true,
            // Batch fallback reconstructs the entire recording from the raw
            // sample ring — any segments emitted before the streaming worker
            // degraded must not be prepended or their text would duplicate.
            full_capture: true,
        })
    }

    /// Write the streaming finalizer's already-trimmed, already-
    /// peak-normalized buffer to a tempfile WAV. Logs incremental-
    /// resample and incremental-vad timings alongside the legacy
    /// `total=` stage time.
    fn write_wav_from_streaming_result(
        &self,
        mut result: FinalizeResult,
        capture_clone_ms: f32,
        streaming_finish_ms: f32,
        t_total_start: Instant,
    ) -> anyhow::Result<StopOutcome> {
        // Full-capture mode: when the backend is fast enough to re-transcribe
        // the whole recording in one pass (Parakeet), prefer the whole-
        // recording buffer over the tail. One pass over the full utterance
        // restores sentence-level punctuation/capitalization context that
        // per-segment transcription destroys (mid-sentence "?" at thinking
        // pauses, stray capitals at segment starts). Segments stay preview-
        // only in this mode. Capped so a marathon dictation doesn't hand the
        // ONNX session an unbounded buffer — beyond the cap we fall back to
        // the segments+tail assembly.
        const FULL_CAPTURE_MAX_SECS: usize = 120;
        let full_capture_wanted = matches!(
            crate::settings::load().backend,
            crate::settings::BackendFamily::Parakeet
        );
        let (trimmed, full_capture) = match result.full_trimmed.take() {
            Some(full)
                if full_capture_wanted
                    && full.len() <= FULL_CAPTURE_MAX_SECS * TARGET_SAMPLE_RATE as usize =>
            {
                tracing::info!(
                    "[audio] full-capture mode: transcribing whole recording \
                     ({:.2}s) in one pass — {} segment(s) become preview-only",
                    full.len() as f32 / TARGET_SAMPLE_RATE as f32,
                    result.segments_emitted,
                );
                (full, true)
            }
            _ => {
                // Tail-only WAV. When no segments were emitted the tail IS
                // the whole recording, so flag it full_capture for accuracy.
                let is_full = result.segments_emitted == 0;
                (std::mem::take(&mut result.trimmed), is_full)
            }
        };

        let min_samples = (TARGET_SAMPLE_RATE as u64 * MIN_RECORDING_MS as u64 / 1000) as usize;
        if trimmed.len() < min_samples {
            let duration_ms = (trimmed.len() as u64 * 1000 / TARGET_SAMPLE_RATE as u64) as u32;
            let total_ms = t_total_start.elapsed().as_secs_f32() * 1000.0;
            tracing::info!(
                "[audio] recording too short after streaming trim ({} samples, {} ms) — skipping",
                trimmed.len(),
                duration_ms
            );
            tracing::info!(
                "[audio] stage timings (ms): capture_clone={:.2} \
                 incremental_resample_total={:.2} incremental_vad_total={:.2} \
                 finalize_flush={:.2} streaming_finish={:.2} \
                 wav_write=0.00 total={:.2} (discarded: too_short, streaming)",
                capture_clone_ms,
                result.incremental_resample_total_ms,
                result.incremental_vad_total_ms,
                result.finalize_flush_ms,
                streaming_finish_ms,
                total_ms,
            );
            return Ok(StopOutcome::Discard(DiscardReason::TooShort {
                duration_ms,
            }));
        }

        let t_wav_start = Instant::now();
        let temp_path = self.write_wav(&trimmed)?;
        let wav_write_ms = t_wav_start.elapsed().as_secs_f32() * 1000.0;

        let total_ms = t_total_start.elapsed().as_secs_f32() * 1000.0;
        tracing::info!(
            "[audio] wrote {} samples ({:.2}s trimmed) → {:?} (streaming, speech_detected={})",
            trimmed.len(),
            trimmed.len() as f32 / TARGET_SAMPLE_RATE as f32,
            temp_path.to_path_buf(),
            result.speech_detected,
        );
        tracing::info!(
            "[audio] stage timings (ms): capture_clone={:.2} \
             incremental_resample_total={:.2} incremental_vad_total={:.2} \
             finalize_flush={:.2} streaming_finish={:.2} \
             wav_write={:.2} total={:.2} (streaming, vad_frames={}, resampled_total={})",
            capture_clone_ms,
            result.incremental_resample_total_ms,
            result.incremental_vad_total_ms,
            result.finalize_flush_ms,
            streaming_finish_ms,
            wav_write_ms,
            total_ms,
            result.vad_frames,
            result.resampled_total,
        );
        Ok(StopOutcome::Wav {
            path: temp_path,
            speech_detected: result.speech_detected,
            full_capture,
        })
    }

    /// Common WAV-write helper for both the streaming and batch paths.
    /// Pulled out so the spec stays in one place — the on-disk handoff
    /// to whisper-cli is invariant: 16 kHz mono 16-bit PCM int.
    fn write_wav(&self, samples: &[f32]) -> anyhow::Result<TempPath> {
        let named = tempfile::Builder::new()
            .prefix("turbotalk-")
            .suffix(".wav")
            .tempfile()?;
        let temp_path: TempPath = named.into_temp_path();
        let path_buf = temp_path.to_path_buf();

        write_transcription_wav(&path_buf, samples)?;
        Ok(temp_path)
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        // Tell the watchdog to exit on its next tick, then join it so
        // it can't outlive the Arcs it holds.
        self.shutdown_watchdog.store(true, Ordering::SeqCst);
        if let Some(h) = self.watchdog_handle.lock().take() {
            let _ = h.join();
        }
        // The warm stream is dropped implicitly with the field.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic stereo 48 kHz buffer → downmix to mono → resample to 16 kHz.
    /// Verifies the output length is approximately `input_frames * 16000 / 48000`
    /// (within ±32 samples for FFT resampler latency / chunk padding).
    #[test]
    fn resample_stereo_48k_to_mono_16k() {
        const SRC_RATE: u32 = 48_000;
        const FRAMES: usize = 48_000; // 1 second
        const CHANNELS: u16 = 2;

        // Interleaved stereo: 1 kHz sine on the left, silence on the right.
        let mut stereo = Vec::with_capacity(FRAMES * CHANNELS as usize);
        for i in 0..FRAMES {
            let t = i as f32 / SRC_RATE as f32;
            let s = (2.0 * std::f32::consts::PI * 1_000.0 * t).sin() * 0.5;
            stereo.push(s); // L
            stereo.push(0.0); // R
        }

        let mono = downmix_to_mono(&stereo, CHANNELS);
        assert_eq!(mono.len(), FRAMES, "downmix must preserve frame count");
        // Right channel is silence so mono = left/2.
        for i in 0..FRAMES {
            assert!((mono[i] - stereo[i * 2] * 0.5).abs() < 1e-6);
        }

        let resampled = resample_to_16k(&mono, SRC_RATE).expect("resample ok");
        let ideal = FRAMES * 16_000 / SRC_RATE as usize; // 16_000
        let diff = (resampled.len() as isize - ideal as isize).abs();
        assert!(
            diff <= 32,
            "resampled length {} differs from ideal {} by more than 32 samples",
            resampled.len(),
            ideal
        );
    }

    /// Mono 16 kHz input must be returned unchanged (fast path).
    #[test]
    fn resample_passthrough_at_target_rate() {
        let buf: Vec<f32> = (0..16_000).map(|i| (i as f32) * 0.0001).collect();
        let out = resample_to_16k(&buf, 16_000).expect("resample ok");
        assert_eq!(out.len(), buf.len());
        assert_eq!(out, buf);
    }

    /// Mono input is returned as a copy with `channels = 1`.
    #[test]
    fn downmix_mono_is_identity() {
        let buf: Vec<f32> = vec![0.1, -0.2, 0.3, -0.4];
        let out = downmix_to_mono(&buf, 1);
        assert_eq!(out, buf);
    }

    /// Quiet buffer (peak 0.1) must be boosted to ~target peak (0.89).
    #[test]
    fn peak_normalize_boosts_quiet_buffer() {
        let mut buf: Vec<f32> = vec![0.05, -0.1, 0.08, -0.07, 0.1, -0.02];
        peak_normalize(&mut buf, 0.89);
        let peak = buf.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        assert!(
            (0.88..=0.90).contains(&peak),
            "expected peak in [0.88, 0.90] after boost, got {}",
            peak
        );
    }

    /// The on-disk handoff to whisper-cli must always be 16 kHz mono 16-bit
    /// PCM int. Pin that contract so an accidental edit to the constants or
    /// the spec helper trips the test suite.
    #[test]
    fn whisper_wav_spec_is_16k_mono_16bit_int() {
        let spec = whisper_wav_spec();
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.bits_per_sample, 16);
        assert!(matches!(spec.sample_format, hound::SampleFormat::Int));
        // The constants must agree with the spec — these are the single
        // source of truth referenced throughout `stop()`.
        assert_eq!(TARGET_SAMPLE_RATE, spec.sample_rate);
        assert_eq!(TARGET_CHANNELS, spec.channels);
        assert_eq!(TARGET_BITS_PER_SAMPLE, spec.bits_per_sample);
    }

    /// `MIN_RECORDING_MS = 100` at 16 kHz must be exactly 1600 samples.
    /// Stop() relies on this for the TooShort discard check.
    #[test]
    fn min_recording_ms_in_samples_at_target_rate() {
        let min_samples = (TARGET_SAMPLE_RATE as u64 * MIN_RECORDING_MS as u64 / 1000) as usize;
        assert_eq!(min_samples, 1600);
    }

    /// Pushing more samples than capacity into the pre-roll ring
    /// must keep length pinned at capacity and retain the *latest* values.
    /// This mirrors the cpal callback's per-tick behavior: extend, then
    /// drain the front if over capacity.
    #[test]
    fn preroll_ring_truncates_to_capacity() {
        const CAP: usize = 300;
        let ring: Mutex<VecDeque<f32>> = Mutex::new(VecDeque::with_capacity(CAP));

        // Simulate 10 cpal callbacks of 100 samples each → 1000 total.
        // Each sample's value encodes its global index so we can verify
        // the kept window is the *latest* 300.
        for chunk in 0..10 {
            let data: Vec<f32> = (0..100).map(|i| (chunk * 100 + i) as f32).collect();
            let mut g = ring.lock();
            g.extend(data.iter().copied());
            if g.len() > CAP {
                let drop = g.len() - CAP;
                g.drain(0..drop);
            }
        }

        let g = ring.lock();
        assert_eq!(g.len(), CAP, "ring length must be pinned at capacity");
        // Latest 300 samples have indices 700..1000.
        assert_eq!(g.front().copied(), Some(700.0));
        assert_eq!(g.back().copied(), Some(999.0));
        for (i, &v) in g.iter().enumerate() {
            assert_eq!(v, (700 + i) as f32, "expected latest-window contents");
        }
    }

    /// A partially-filled ring (under capacity) must be returned
    /// in full on drain — represents the cold-start case where the user
    /// presses PTT before the warm stream has run for PREROLL_MS.
    #[test]
    fn preroll_ring_underfilled_drains_all() {
        const CAP: usize = 300;
        let ring: Mutex<VecDeque<f32>> = Mutex::new(VecDeque::with_capacity(CAP));

        // 50 samples total — well under capacity.
        let data: Vec<f32> = (0..50).map(|i| i as f32).collect();
        {
            let mut g = ring.lock();
            g.extend(data.iter().copied());
            if g.len() > CAP {
                let drop = g.len() - CAP;
                g.drain(0..drop);
            }
        }

        let drained: Vec<f32> = ring.lock().drain(..).collect();
        assert_eq!(drained, data);
        assert!(ring.lock().is_empty(), "ring must be empty after drain");
    }

    /// Buffer already louder than target (peak 0.95) must pass through unchanged.
    #[test]
    fn peak_normalize_leaves_loud_buffer_alone() {
        let original: Vec<f32> = vec![0.2, -0.5, 0.95, -0.8, 0.3];
        let mut buf = original.clone();
        peak_normalize(&mut buf, 0.89);
        assert_eq!(buf, original, "loud buffer must not be attenuated");
        let peak = buf.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
        assert!(
            (peak - 0.95).abs() < f32::EPSILON,
            "peak should remain 0.95, got {}",
            peak
        );
    }
}
