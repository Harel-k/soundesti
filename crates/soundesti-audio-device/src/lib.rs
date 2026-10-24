//! Hosts the [`Engine`] on a real audio device (CPAL; WASAPI shared mode on Windows),
//! or on a silent real-time clock when no device is available.
//!
//! The engine's sample rate must match the device, so [`start`] picks the device
//! configuration first and then creates the engine, returning its control handle.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use soundesti_audio::{Engine, EngineConfig, EngineHandle};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

/// What the user asked for. `None` fields mean "device default".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputSettings {
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_frames: Option<u32>,
}

/// What the device actually gave us.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamInfo {
    pub host: String,
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// Requested fixed buffer size, if any (None = driver default).
    pub buffer_frames: Option<u32>,
    pub sample_format: String,
    /// True when running on the silent fallback clock instead of a device.
    pub is_null: bool,
}

impl StreamInfo {
    /// Output latency implied by the buffer size, if known.
    pub fn buffer_latency_ms(&self) -> Option<f64> {
        self.buffer_frames.map(|f| f as f64 * 1000.0 / self.sample_rate as f64)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("no audio output device found")]
    NoDevice,
    #[error("output device `{0}` not found")]
    DeviceNotFound(String),
    #[error("audio device error: {0}")]
    Backend(String),
}

/// Stream health reported by the backend's error callback.
#[derive(Default)]
pub struct StreamHealth {
    /// The stream died (device unplugged, invalidated). The UI should offer a restart.
    pub failed: AtomicBool,
    pub errors: AtomicU64,
}

pub fn list_output_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default = host.default_output_device().map(|d| d.to_string());
    host.output_devices()
        .map(|devs| {
            devs.map(|d| {
                let name = d.to_string();
                DeviceInfo { is_default: Some(&name) == default.as_ref(), name }
            })
            .collect()
        })
        .unwrap_or_default()
}

enum Backend {
    Device(#[allow(dead_code)] cpal::Stream),
    Null { stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> },
}

/// A running output. Dropping it stops audio.
pub struct AudioOutput {
    backend: Backend,
    pub info: StreamInfo,
    pub health: Arc<StreamHealth>,
    /// Configuration the hosted engine was created with.
    pub engine_config: EngineConfig,
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        if let Backend::Null { stop, thread } = &mut self.backend {
            stop.store(true, Ordering::Relaxed);
            if let Some(t) = thread.take() {
                let _ = t.join();
            }
        }
    }
}

const MAX_BLOCK: usize = 512;
const MAX_CHANNEL_SLOTS: usize = 256;
/// Scratch for non-f32 devices: enough for large WASAPI buffers; bigger callbacks are split.
const SCRATCH_FRAMES: usize = 16_384;

/// Opens the requested device and starts the engine on it.
pub fn start(settings: &OutputSettings) -> Result<(AudioOutput, EngineHandle), DeviceError> {
    let host = cpal::default_host();
    let device = match &settings.device {
        Some(name) => host
            .output_devices()
            .map_err(|e| DeviceError::Backend(e.to_string()))?
            .find(|d| &d.to_string() == name)
            .ok_or_else(|| DeviceError::DeviceNotFound(name.clone()))?,
        None => host.default_output_device().ok_or(DeviceError::NoDevice)?,
    };
    let default = device.default_output_config().map_err(|e| DeviceError::Backend(e.to_string()))?;
    let sample_format = default.sample_format();
    let mut config = default.config();
    if let Some(sr) = settings.sample_rate {
        let supported = device
            .supported_output_configs()
            .map(|mut it| {
                it.any(|r| r.sample_format() == sample_format && r.min_sample_rate() <= sr && sr <= r.max_sample_rate())
            })
            .unwrap_or(false);
        if supported {
            config.sample_rate = sr;
        } else {
            log::warn!("sample rate {sr} not supported by {device}; using {}", config.sample_rate);
        }
    }
    let mut buffer_frames = None;
    if let Some(frames) = settings.buffer_frames {
        let ok = match default.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => (*min..=*max).contains(&frames),
            cpal::SupportedBufferSize::Unknown => true,
        };
        if ok {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
            buffer_frames = Some(frames);
        }
    }

    let engine_config =
        EngineConfig { sample_rate: config.sample_rate as f64, max_block: MAX_BLOCK, max_channels: MAX_CHANNEL_SLOTS };
    let (engine, handle) = Engine::new(engine_config);
    let health = Arc::new(StreamHealth::default());
    let channels = config.channels as usize;

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_f32(&device, &config, engine, channels, &health),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, engine, channels, &health),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, engine, channels, &health),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, engine, channels, &health),
        other => return Err(DeviceError::Backend(format!("unsupported device sample format {other}"))),
    }?;
    stream.play().map_err(|e| DeviceError::Backend(e.to_string()))?;

    let info = StreamInfo {
        host: format!("{:?}", host.id()),
        device: device.to_string(),
        sample_rate: config.sample_rate,
        channels: config.channels,
        buffer_frames,
        sample_format: sample_format.to_string(),
        is_null: false,
    };
    Ok((AudioOutput { backend: Backend::Device(stream), info, health, engine_config }, handle))
}

fn error_callback(health: &Arc<StreamHealth>) -> impl FnMut(cpal::Error) + Send + 'static {
    let health = health.clone();
    move |e: cpal::Error| {
        log::error!("audio stream error: {e}");
        health.errors.fetch_add(1, Ordering::Relaxed);
        health.failed.store(true, Ordering::Relaxed);
    }
}

/// Native float devices: the engine renders straight into the device buffer.
fn build_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut engine: Engine,
    channels: usize,
    health: &Arc<StreamHealth>,
) -> Result<cpal::Stream, DeviceError> {
    device
        .build_output_stream::<f32, _, _>(
            *config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| engine.process(data, channels),
            error_callback(health),
            Some(Duration::from_secs(3)),
        )
        .map_err(|e| DeviceError::Backend(e.to_string()))
}

/// Integer devices: render into a preallocated f32 scratch buffer, then convert.
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut engine: Engine,
    channels: usize,
    health: &Arc<StreamHealth>,
) -> Result<cpal::Stream, DeviceError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut scratch = vec![0.0f32; SCRATCH_FRAMES * channels];
    let data_cb = move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
        for chunk in data.chunks_mut(scratch.len()) {
            let buf = &mut scratch[..chunk.len()];
            engine.process(buf, channels);
            for (o, &s) in chunk.iter_mut().zip(buf.iter()) {
                *o = T::from_sample(s.clamp(-1.0, 1.0));
            }
        }
    };
    device
        .build_output_stream::<T, _, _>(*config, data_cb, error_callback(health), Some(Duration::from_secs(3)))
        .map_err(|e| DeviceError::Backend(e.to_string()))
}

/// Runs the engine on a silent real-time clock. Used when no device can be opened, so
/// the transport, meters and editing keep working (and the UI says audio is off).
pub fn start_null(sample_rate: u32) -> (AudioOutput, EngineHandle) {
    let config =
        EngineConfig { sample_rate: sample_rate as f64, max_block: MAX_BLOCK, max_channels: MAX_CHANNEL_SLOTS };
    let (mut engine, handle) = Engine::new(config);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let thread = std::thread::Builder::new()
        .name("soundesti-null-audio".into())
        .spawn(move || {
            let frames = 256usize;
            let mut buf = vec![0.0f32; frames * 2];
            let period = Duration::from_secs_f64(frames as f64 / sample_rate as f64);
            let mut next = Instant::now();
            while !stop_t.load(Ordering::Relaxed) {
                engine.process(&mut buf, 2);
                next += period;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    next = now;
                }
            }
        })
        .expect("spawn null audio thread");
    let info = StreamInfo {
        host: "None".into(),
        device: "No audio device (silent)".into(),
        sample_rate,
        channels: 2,
        buffer_frames: Some(256),
        sample_format: "f32".into(),
        is_null: true,
    };
    (
        AudioOutput {
            backend: Backend::Null { stop, thread: Some(thread) },
            info,
            health: Arc::new(StreamHealth::default()),
            engine_config: config,
        },
        handle,
    )
}

/// Tries the requested device, then the default device, then the silent clock.
pub fn start_or_fallback(settings: &OutputSettings) -> (AudioOutput, EngineHandle, Option<DeviceError>) {
    match start(settings) {
        Ok((o, h)) => (o, h, None),
        Err(first) => {
            if settings != &OutputSettings::default()
                && let Ok((o, h)) = start(&OutputSettings::default())
            {
                return (o, h, Some(first));
            }
            let (o, h) = start_null(48_000);
            (o, h, Some(first))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundesti_audio::Command;

    #[test]
    fn null_output_advances_transport() {
        let (out, mut handle) = start_null(48_000);
        assert!(out.info.is_null);
        handle.send(Command::Play);
        std::thread::sleep(Duration::from_millis(300));
        let pos = handle.shared().position_tick();
        // ~0.3 s at 120 BPM = ~576 ticks; allow scheduler jitter.
        assert!(pos > 200.0 && pos < 1200.0, "position {pos}");
        drop(out);
    }

    #[test]
    fn missing_device_is_reported() {
        let s = OutputSettings { device: Some("definitely-not-a-real-device-xyz".into()), ..Default::default() };
        assert!(matches!(start(&s), Err(DeviceError::DeviceNotFound(_)) | Err(DeviceError::Backend(_))));
    }

    /// Opens the real default device for half a second. Run manually on machines with audio:
    /// `cargo test -p soundesti-audio-device -- --ignored`.
    #[test]
    #[ignore]
    fn real_device_plays() {
        let (out, mut handle) = start(&OutputSettings::default()).expect("open default device");
        handle.send(Command::SetMetronome(true));
        handle.send(Command::Play);
        std::thread::sleep(Duration::from_millis(600));
        assert!(handle.shared().callbacks.load(Ordering::Relaxed) > 0, "no audio callbacks on {}", out.info.device);
        assert!(!out.health.failed.load(Ordering::Relaxed));
    }
}
