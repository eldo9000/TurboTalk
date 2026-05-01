use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct ActiveRecording {
    stop_signal: Arc<AtomicBool>,
    join: JoinHandle<anyhow::Result<PathBuf>>,
}

pub fn start() -> anyhow::Result<ActiveRecording> {
    let stop_signal = Arc::new(AtomicBool::new(false));
    let stop_thread = stop_signal.clone();
    let join = std::thread::spawn(move || run_stream(stop_thread));
    Ok(ActiveRecording { stop_signal, join })
}

pub fn stop(rec: ActiveRecording) -> anyhow::Result<PathBuf> {
    rec.stop_signal.store(true, Ordering::SeqCst);
    rec.join
        .join()
        .map_err(|_| anyhow::anyhow!("audio thread panicked"))?
}

fn run_stream(stop: Arc<AtomicBool>) -> anyhow::Result<PathBuf> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("no default input device"))?;
    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let sample_format = config.sample_format();

    let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(sample_rate as usize)));
    let samples_cb = samples.clone();
    let err_fn = |e| tracing::error!("[audio] stream error: {:?}", e);

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| samples_cb.lock().extend_from_slice(data),
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                let mut buf = samples_cb.lock();
                buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config.into(),
            move |data: &[u16], _: &_| {
                let mut buf = samples_cb.lock();
                buf.extend(data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0));
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow::anyhow!("unsupported sample format: {:?}", other)),
    };
    stream.play()?;
    tracing::info!(
        "[audio] capturing — {} Hz, {} ch, {:?}",
        sample_rate,
        channels,
        sample_format
    );

    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(stream);

    let buf = samples.lock();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("turbotalk-{}.wav", stamp));
    let spec = hound::WavSpec {
        channels,
        sample_rate,
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
