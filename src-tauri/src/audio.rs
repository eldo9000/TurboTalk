// Keep one CoreAudio stream running at all times (started at app launch).
// On start(): flip is_recording true and clear the buffer — zero hardware latency.
// On stop(): flip false, drain the buffer, write WAV.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct AudioCapture {
    _stream: cpal::Stream,
    is_recording: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

// _stream is held only to keep CoreAudio alive — never accessed across threads.
// All mutable state (is_recording, samples) is already thread-safe.
unsafe impl Send for AudioCapture {}
unsafe impl Sync for AudioCapture {}

impl AudioCapture {
    pub fn new() -> anyhow::Result<Self> {
        let cfg = crate::settings::load();
        let want = cfg.audio.device.as_str();
        let host = cpal::default_host();
        let device = if want == "default" || want.is_empty() {
            host.default_input_device()
                .ok_or_else(|| anyhow::anyhow!("no default input device"))?
        } else {
            host.input_devices()?
                .find(|d| d.name().ok().as_deref() == Some(want))
                .or_else(|| host.default_input_device())
                .ok_or_else(|| anyhow::anyhow!("no input device found"))?
        };
        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let sample_format = config.sample_format();

        let is_recording = Arc::new(AtomicBool::new(false));
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));

        let rec_flag = is_recording.clone();
        let samples_cb = samples.clone();
        let err_fn = |e| tracing::error!("[audio] stream error: {:?}", e);

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &_| {
                    if rec_flag.load(Ordering::Relaxed) {
                        samples_cb.lock().extend_from_slice(data);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &_| {
                    if rec_flag.load(Ordering::Relaxed) {
                        let mut buf = samples_cb.lock();
                        buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _: &_| {
                    if rec_flag.load(Ordering::Relaxed) {
                        let mut buf = samples_cb.lock();
                        buf.extend(data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0));
                    }
                },
                err_fn,
                None,
            )?,
            other => anyhow::bail!("unsupported sample format: {:?}", other),
        };

        stream.play()?;
        tracing::info!(
            "[audio] stream warmed — {} Hz, {} ch, {:?}",
            sample_rate, channels, sample_format
        );

        Ok(Self { _stream: stream, is_recording, samples, sample_rate, channels })
    }

    pub fn start(&self) {
        self.samples.lock().clear();
        self.is_recording.store(true, Ordering::SeqCst);
        tracing::info!("[audio] recording started");
    }

    pub fn stop(&self) -> anyhow::Result<PathBuf> {
        self.is_recording.store(false, Ordering::SeqCst);
        // Let the last in-flight callback finish (CoreAudio buffer ≈ 10ms)
        std::thread::sleep(Duration::from_millis(25));

        let buf = self.samples.lock().clone();
        tracing::info!("[audio] {} samples captured", buf.len());

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("turbotalk-{}.wav", stamp));
        let spec = hound::WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec)?;
        for &s in buf.iter() {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(v)?;
        }
        writer.finalize()?;
        tracing::info!("[audio] wrote {} samples → {:?}", buf.len(), path);
        Ok(path)
    }
}
