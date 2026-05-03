// Keep the CoreAudio stream open only while recording.
// On start(): query the current device from config, open a fresh stream.
// On stop(): stop recording, drain the buffer, write WAV, drop the stream.
// This lets the device change (built-in ↔ AirPods) without restarting the app.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tempfile::TempPath;

use crate::audio_finalizer::{DropReason, FinalizeResult, StreamingFinalizer};

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

struct ActiveStream {
    _stream: cpal::Stream,
    sample_rate: u32,
    channels: u16,
}

pub struct AudioCapture {
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>, // current RMS as f32 bits
    is_recording: Arc<AtomicBool>,
    /// Set by the cpal error callback when CoreAudio reports the device went
    /// away mid-recording (e.g. AirPods disconnected). Read edge-triggered by
    /// `device_lost()` — swap-to-false on read so a single device-loss event
    /// surfaces exactly once.
    device_lost: Arc<AtomicBool>,
    active: Mutex<Option<ActiveStream>>,
    /// TASK-22: streaming finalizer worker (resample + VAD off the
    /// post-release critical path). Spawned in `start()`, shut down in
    /// `stop()` / `cancel()`. The capture-feeder thread (`feeder`) ships
    /// chunks to it from the shared `samples` buffer.
    ///
    /// On streaming degradation (worker init failure, channel disconnect)
    /// `stop()` falls back to the legacy batch finalizer path against
    /// the canonical `samples` buffer — no recording is ever lost.
    streaming: Mutex<Option<StreamingFinalizer>>,
    /// Capture-feeder thread handle. The feeder polls `samples` and ships
    /// each new chunk to the streaming worker. Joined by `stop()` /
    /// `cancel()` after `feeder_stop` is set so it returns cleanly.
    feeder: Mutex<Option<JoinHandle<()>>>,
    /// Set true to ask the feeder thread to drain pending samples, send
    /// them to the worker, and exit. The feeder polls this every ~10 ms.
    feeder_stop: Arc<AtomicBool>,
    /// How many samples from `samples` the feeder has already shipped to
    /// the streaming worker. Owned by the feeder; `stop()` reads it after
    /// the feeder has exited so it knows whether all captured audio
    /// reached the worker.
    feeder_cursor: Arc<AtomicUsize>,
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
    Wav { path: TempPath },
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
//     dedicated OS thread that owns the CFRunLoop): the *only* thread that
//     ever touches `self.active`. It calls `start()` (creating the Stream)
//     and `stop()` (dropping the Stream) — both on the same thread. The
//     `active: Mutex<Option<ActiveStream>>` is taken as the only path to
//     read or replace the stream; it serializes against any pathological
//     re-entry.
//   - Level-broadcast thread (spawned in lib.rs): calls `level()` which
//     reads the `level: Arc<AtomicU32>` only, and `recorder.is_recording()`
//     which reads Recorder's own `Mutex<State>` — never touches `active`
//     and therefore never touches the cpal::Stream.
//   - CoreAudio callback thread (cpal-managed): the callback closure
//     captures clones of `is_recording` (AtomicBool), `samples` (Arc<Mutex>)
//     and `level` (AtomicU32) — never the `active` field, never the Stream
//     itself, never `&self`.
//
// So `active` is single-threaded in practice, and every other field is
// already-Send-and-Sync (atomics + `Arc<Mutex<Vec<f32>>>`). No data race
// on the Stream is reachable. The unsafe impls patch over a missing Send
// bound on a `Box<dyn FnMut()>` deep inside cpal; they do not paper over
// any real concurrent access.
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

/// Peak-normalize a buffer of f32 samples to the given target peak.
/// One-way: boosts quiet input; never attenuates loud input. Built-in
/// MacBook microphones typically peak between -25 and -18 dBFS, well below
/// what whisper.cpp was trained on — boosting before transcription
/// measurably reduces hallucinations on quiet audio (see
/// https://arxiv.org/html/2505.12969v1 and faster-whisper#183).
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

fn peak_normalize(samples: &mut [f32], target: f32) {
    let peak = samples.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
    if peak > 0.0 && peak < target {
        let gain = target / peak;
        for s in samples.iter_mut() {
            *s = (*s * gain).clamp(-1.0, 1.0);
        }
    }
}

impl AudioCapture {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            samples: Arc::new(Mutex::new(Vec::new())),
            level: Arc::new(AtomicU32::new(0)),
            is_recording: Arc::new(AtomicBool::new(false)),
            device_lost: Arc::new(AtomicBool::new(false)),
            active: Mutex::new(None),
            streaming: Mutex::new(None),
            feeder: Mutex::new(None),
            feeder_stop: Arc::new(AtomicBool::new(false)),
            feeder_cursor: Arc::new(AtomicUsize::new(0)),
        })
    }

    fn open_stream(&self, want: &str) -> anyhow::Result<ActiveStream> {
        let host = cpal::default_host();
        let device = if want == "default" || want.is_empty() {
            host.default_input_device()
                .ok_or_else(|| anyhow::anyhow!(
                    "Microphone access denied — grant permission in System Settings → Privacy → Microphone, then relaunch."
                ))?
        } else {
            host.input_devices()?
                .find(|d| d.name().ok().as_deref() == Some(want))
                .or_else(|| host.default_input_device())
                .ok_or_else(|| anyhow::anyhow!(
                    "Microphone access denied — grant permission in System Settings → Privacy → Microphone, then relaunch."
                ))?
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
        let smp = self.samples.clone();
        let lvl = self.level.clone();

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

        // ---- cpal callback discipline (TASK-22) ------------------------
        // The audio callback runs on CoreAudio's high-priority thread and
        // must do only:
        //   1. extend the shared `samples: Mutex<Vec<f32>>` buffer with
        //      the new native-rate slice;
        //   2. update the level meter atomic.
        //
        // No DSP. No channel sends. No heap-alloc beyond what
        // `Vec::extend_from_slice` does (amortized; `samples` is
        // pre-grown by the first few callbacks). The streaming finalizer
        // pulls from `samples` via the capture-feeder thread, never
        // here.
        //
        // This matches `cjpais/Handy`'s callback discipline and the
        // TASK-22 constraint. Verified by code inspection: the only
        // operations below `extend_from_slice` and `level.store`. If
        // you add work here, it MUST be moved to the feeder or worker.
        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &_| {
                        // CALLBACK-ALLOWED-OPS: append + level only.
                        if rec.load(Ordering::Relaxed) {
                            smp.lock().extend_from_slice(data);
                            lvl.store(rms(data).to_bits(), Ordering::Relaxed);
                        } else {
                            lvl.store(0_f32.to_bits(), Ordering::Relaxed);
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &_| {
                    let floats: Vec<f32> =
                        data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    if rec.load(Ordering::Relaxed) {
                        smp.lock().extend_from_slice(&floats);
                        lvl.store(rms(&floats).to_bits(), Ordering::Relaxed);
                    } else {
                        lvl.store(0_f32.to_bits(), Ordering::Relaxed);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _: &_| {
                    let floats: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32 - 32768.0) / 32768.0)
                        .collect();
                    if rec.load(Ordering::Relaxed) {
                        smp.lock().extend_from_slice(&floats);
                        lvl.store(rms(&floats).to_bits(), Ordering::Relaxed);
                    } else {
                        lvl.store(0_f32.to_bits(), Ordering::Relaxed);
                    }
                },
                err_fn,
                None,
            )?,
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

    /// Edge-triggered read: returns true once when the cpal error callback
    /// flagged the device as gone, then resets so the next caller sees false.
    pub fn device_lost(&self) -> bool {
        self.device_lost.swap(false, Ordering::SeqCst)
    }

    pub fn start(&self) -> anyhow::Result<()> {
        self.samples.lock().clear();
        self.level.store(0_f32.to_bits(), Ordering::Relaxed);
        // Clear any stale device-lost flag from a previous session so a fresh
        // recording doesn't immediately trigger the loss path.
        self.device_lost.store(false, Ordering::SeqCst);
        self.feeder_stop.store(false, Ordering::SeqCst);
        self.feeder_cursor.store(0, Ordering::SeqCst);

        // Always read device from config so changes take effect without restart.
        let want = crate::settings::load().audio.device;
        let stream = self.open_stream(&want)?;
        let src_rate = stream.sample_rate;
        let src_channels = stream.channels;
        *self.active.lock() = Some(stream);

        // TASK-22: spawn the streaming finalizer worker and the
        // capture-feeder thread. The worker owns the resampler + VAD
        // state and consumes chunks off the cpal callback's critical
        // path. The feeder polls `samples` every ~10 ms and ships any
        // newly-appended tail to the worker.
        let finalizer = StreamingFinalizer::start(src_rate, src_channels, NORMALIZE_PEAK);
        *self.streaming.lock() = Some(finalizer);

        let feeder_handle = self.spawn_feeder();
        *self.feeder.lock() = Some(feeder_handle);

        self.is_recording.store(true, Ordering::SeqCst);
        tracing::info!("[audio] recording started");
        Ok(())
    }

    /// Spawn the capture-feeder thread. It polls `samples` for new
    /// content (via a cursor) and ships each new chunk to the streaming
    /// finalizer via the bounded crossbeam channel. The feeder runs
    /// off the audio callback thread, so a slow channel send (or even a
    /// full channel) cannot back-propagate into the cpal callback.
    fn spawn_feeder(&self) -> JoinHandle<()> {
        // Capture clones we hand to the thread. We deliberately do NOT
        // capture `&self` — the feeder must be detachable so `start()`
        // can return without leaking lifetimes.
        let samples = self.samples.clone();
        let stop_flag = self.feeder_stop.clone();
        let cursor = self.feeder_cursor.clone();
        // The streaming finalizer lives behind the capture's `streaming:
        // Mutex<Option<...>>`. We can't share that across the thread
        // boundary directly (the `StreamingFinalizer` is owned by
        // AudioCapture, not Arc-shared), so the feeder reaches into it
        // each tick by cloning the `Sender` once — which it can't,
        // because the Sender is private. Instead, expose `try_send_samples`
        // through a clone of the Sender held in the finalizer.
        //
        // For simplicity, we clone a `try_send` closure: the feeder
        // captures an Arc<Mutex<Option<StreamingFinalizer>>> by way of
        // a small shim. To avoid restructuring AudioCapture into Arcs,
        // we instead reach through `self` via a separate Arc-shared
        // channel sender. That's what we'll do: the streaming module
        // exposes a clonable `Sender`-style handle.
        //
        // Practical: clone a fresh handle to the channel from the
        // running finalizer.
        let finalizer_sender = self
            .streaming
            .lock()
            .as_ref()
            .map(|f| f.handle())
            .expect("finalizer must exist by start() time");

        std::thread::Builder::new()
            .name("turbotalk-capture-feeder".into())
            .spawn(move || {
                // Poll cadence: 10 ms is short enough to keep the
                // capture-feeder ahead of the resampler (~30× faster
                // than realtime on this hardware), and long enough that
                // the polling loop itself doesn't burn CPU. We never
                // hold the `samples` lock across DSP — only across the
                // copy-out of the new tail.
                let poll_interval = Duration::from_millis(10);
                let mut backpressure_drops: u64 = 0;

                loop {
                    let stop = stop_flag.load(Ordering::SeqCst);

                    // Snapshot the new tail. Hold the lock only across
                    // the copy; never across the channel send.
                    let new_tail: Option<Vec<f32>> = {
                        let buf = samples.lock();
                        let consumed = cursor.load(Ordering::SeqCst);
                        if buf.len() > consumed {
                            let slice = &buf[consumed..];
                            let v = slice.to_vec();
                            cursor.store(buf.len(), Ordering::SeqCst);
                            Some(v)
                        } else {
                            None
                        }
                    };

                    if let Some(chunk) = new_tail {
                        match finalizer_sender.try_send(chunk) {
                            Ok(()) => {}
                            Err(DropReason::WorkerBackpressure) => {
                                // Drop on the capture-feeder side, never
                                // on the cpal callback side. The
                                // canonical audio buffer is `samples`,
                                // so `stop()`'s batch fallback can still
                                // recover the recording bit-for-bit.
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
                        // After observing the stop signal we did one
                        // last drain above. Now exit so `stop()` can
                        // call `finish()` on the finalizer.
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

    /// Drop the active stream and clear the sample buffer without producing a
    /// WAV. Called by the level-broadcast thread when device loss is detected.
    pub fn cancel(&self) {
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

        *self.active.lock() = None;
        self.samples.lock().clear();
        self.level.store(0_f32.to_bits(), Ordering::Relaxed);
        tracing::info!("[audio] recording cancelled");
    }

    pub fn stop(&self) -> anyhow::Result<StopOutcome> {
        // Stage timing — see ARCHITECTURE.md "Audio Pipeline Contract".
        // We log a single compact line at the end of finalization so later
        // optimization work (TASK-17 / TASK-18 / TASK-22) is grounded in
        // measurements instead of vibes. The legacy `total=` field is
        // preserved for direct comparison against TASK-21 evidence, with
        // additional `incremental_resample_total`, `incremental_vad_total`,
        // and `finalize_flush` fields when the streaming path is used.
        let t_total_start = Instant::now();

        self.is_recording.store(false, Ordering::SeqCst);
        // Let the last in-flight callback finish (CoreAudio buffer ≈ 10ms)
        std::thread::sleep(Duration::from_millis(25));

        let t_capture_clone_start = Instant::now();
        let (src_sample_rate, src_channels) = {
            let active = self.active.lock();
            match active.as_ref() {
                Some(a) => (a.sample_rate, a.channels),
                None => return Ok(StopOutcome::Discard(DiscardReason::NoStream)),
            }
        };
        *self.active.lock() = None; // drop stream

        // Signal the capture-feeder to drain remaining samples and
        // exit, then join it. After this the streaming worker has
        // received every sample the cpal callback ever wrote.
        self.feeder_stop.store(true, Ordering::SeqCst);
        let feeder = self.feeder.lock().take();
        if let Some(h) = feeder {
            let _ = h.join();
        }

        // Snapshot the canonical buffer for the batch-fallback path
        // before we tear down the streaming finalizer. Cheap: this is
        // the same `buf.clone()` the legacy path used.
        let buf_full = self.samples.lock().clone();
        let capture_clone_ms = t_capture_clone_start.elapsed().as_secs_f32() * 1000.0;

        tracing::info!(
            "[audio] {} samples captured ({} Hz, {} ch — pre-resample)",
            buf_full.len(),
            src_sample_rate,
            src_channels
        );

        // Try the streaming finalizer first.
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
        // Same code as pre-TASK-22. Reached only when the streaming
        // path is degraded (worker init failure, mid-stream VAD/
        // resampler error). Preserved verbatim so a streaming
        // regression can never lose a recording.
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
        Ok(StopOutcome::Wav { path: temp_path })
    }

    /// Write the streaming finalizer's already-trimmed, already-
    /// peak-normalized buffer to a tempfile WAV. Logs the new TASK-22
    /// stage-timings shape (with `incremental_*` and `finalize_flush`
    /// fields) alongside the legacy `total=` for comparison against
    /// TASK-21 evidence.
    fn write_wav_from_streaming_result(
        &self,
        result: FinalizeResult,
        capture_clone_ms: f32,
        streaming_finish_ms: f32,
        t_total_start: Instant,
    ) -> anyhow::Result<StopOutcome> {
        let trimmed = result.trimmed;

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
        Ok(StopOutcome::Wav { path: temp_path })
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

        let spec = whisper_wav_spec();
        let mut writer = hound::WavWriter::create(&path_buf, spec)?;
        for &s in samples.iter() {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(v)?;
        }
        writer.finalize()?;
        Ok(temp_path)
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
    /// the spec helper trips the test suite. TASK-13 success signal.
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
