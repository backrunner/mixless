use std::sync::atomic::Ordering;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::engine::{AudioRt, Shared};

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("no output device")]
    NoDevice,
    #[error("audio: {0}")]
    Cpal(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub master_device: Option<String>,
    pub cue_device: Option<String>,
    /// None uses the selected device's native sample rate / buffer size.
    pub sample_rate: Option<u32>,
    pub buffer_frames: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub sample_rates: Vec<u32>,
}

pub fn output_devices() -> Result<Vec<AudioDevice>, DeviceError> {
    cpal::default_host()
        .output_devices()
        .map_err(audio_error)?
        .map(|device| {
            let name = device.name().map_err(audio_error)?;
            let configs: Vec<_> = device
                .supported_output_configs()
                .map_err(audio_error)?
                .collect();
            let sample_rates = [44_100, 48_000, 88_200, 96_000]
                .into_iter()
                .filter(|rate| {
                    configs.iter().any(|c| {
                        supported_format(c.sample_format())
                            && c.min_sample_rate().0 <= *rate
                            && c.max_sample_rate().0 >= *rate
                    })
                })
                .collect();
            Ok(AudioDevice { name, sample_rates })
        })
        .collect()
}

fn audio_error(error: impl std::fmt::Display) -> DeviceError {
    DeviceError::Cpal(error.to_string())
}

fn supported_format(format: cpal::SampleFormat) -> bool {
    matches!(format, cpal::SampleFormat::F32 | cpal::SampleFormat::I16)
}

fn find_device(name: Option<&str>) -> Result<cpal::Device, DeviceError> {
    let host = cpal::default_host();
    match name {
        None => host.default_output_device().ok_or(DeviceError::NoDevice),
        Some(name) => host
            .output_devices()
            .map_err(audio_error)?
            .find(|d| d.name().ok().as_deref() == Some(name))
            .ok_or_else(|| audio_error(format!("device unavailable: {name}"))),
    }
}

fn stream_config(
    device: &cpal::Device,
    rate: Option<u32>,
    buffer: Option<u32>,
) -> Result<(cpal::StreamConfig, cpal::SampleFormat), DeviceError> {
    let default = device.default_output_config().map_err(audio_error)?;
    let rate = rate.unwrap_or(default.sample_rate().0);
    if !(8_000..=96_000).contains(&rate) {
        return Err(audio_error("sample rate must be between 8000 and 96000 Hz"));
    }
    let supported = device
        .supported_output_configs()
        .map_err(audio_error)?
        .filter(|c| {
            supported_format(c.sample_format())
                && c.min_sample_rate().0 <= rate
                && c.max_sample_rate().0 >= rate
        })
        .min_by_key(|c| {
            (
                c.channels() != default.channels(),
                c.sample_format() != cpal::SampleFormat::F32,
            )
        })
        .ok_or_else(|| audio_error(format!("device does not support {rate} Hz")))?;
    if let Some(frames) = buffer {
        if frames == 0 || frames > 8192 {
            return Err(audio_error("invalid buffer size"));
        }
        if let cpal::SupportedBufferSize::Range { min, max } = supported.buffer_size() {
            if frames < *min || frames > *max {
                return Err(audio_error(format!(
                    "buffer size must be {min} to {max} frames"
                )));
            }
        }
    }
    let format = supported.sample_format();
    let mut config = supported.with_sample_rate(cpal::SampleRate(rate)).config();
    config.buffer_size = buffer.map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed);
    Ok((config, format))
}

pub(crate) struct OutputStreams {
    master: cpal::Stream,
    cue: Option<cpal::Stream>,
    pub sample_rate: u32,
    pub device_name: String,
    pub cue_name: Option<String>,
}

impl OutputStreams {
    pub fn prepare(shared: Arc<Shared>, settings: &AudioConfig) -> Result<Self, DeviceError> {
        let master_device = find_device(settings.master_device.as_deref())?;
        let device_name = master_device.name().map_err(audio_error)?;
        if settings.cue_device.as_ref() == Some(&device_name) {
            return Err(audio_error(
                "headphone output must be a different device from master",
            ));
        }
        let (config, format) =
            stream_config(&master_device, settings.sample_rate, settings.buffer_frames)?;
        let mut rt = AudioRt::new(config.sample_rate.0 as f32);
        let cue = if let Some(name) = &settings.cue_device {
            let device = find_device(Some(name))?;
            let (cue_config, cue_format) = stream_config(&device, None, None)?;
            let (producer, mut consumer) = rtrb::RingBuffer::<[f32; 2]>::new(8192);
            rt.cue_output = Some(CueWriter::new(
                producer,
                config.sample_rate.0,
                cue_config.sample_rate.0,
            ));
            let cue_shared = shared.clone();
            Some(build_stream(
                &device,
                &cue_config,
                cue_format,
                move |data, channels, _playback| {
                    // Independent device clocks can drift. Bound accumulated latency.
                    if consumer.slots() > 4096 {
                        while consumer.slots() > 1024 {
                            let _ = consumer.pop();
                        }
                    }
                    for frame in data.chunks_mut(channels) {
                        route_stereo(frame, consumer.pop().unwrap_or([0.0; 2]));
                    }
                },
                move |_| {
                    cue_shared.cue_failed.store(true, Ordering::Relaxed);
                },
            )?)
        } else {
            None
        };
        let error_shared = shared.clone();
        let master = build_stream(
            &master_device,
            &config,
            format,
            move |data, channels, playback| {
                shared
                    .block_frames
                    .store((data.len() / channels) as u32, Ordering::Relaxed);
                Shared::render_interleaved(&shared, &mut rt, data, channels, playback);
            },
            move |_| {
                error_shared.audio_failed.store(true, Ordering::Relaxed);
            },
        )?;
        Ok(Self {
            master,
            cue,
            sample_rate: config.sample_rate.0,
            device_name,
            cue_name: settings.cue_device.clone(),
        })
    }

    pub fn play(&self) -> Result<(), DeviceError> {
        if let Some(cue) = &self.cue {
            cue.play().map_err(audio_error)?;
        }
        if let Err(error) = self.master.play() {
            if let Some(cue) = &self.cue {
                let _ = cue.pause();
            }
            return Err(audio_error(error));
        }
        Ok(())
    }

    pub fn pause(&self) -> Result<(), DeviceError> {
        self.master.pause().map_err(audio_error)?;
        if let Some(cue) = &self.cue {
            let _ = cue.pause();
        }
        Ok(())
    }
}

fn route_stereo(frame: &mut [f32], sample: [f32; 2]) {
    frame.fill(0.0);
    if frame.len() == 1 {
        frame[0] = (sample[0] + sample[1]) * 0.5;
    } else {
        frame[..2].copy_from_slice(&sample);
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    mut render: impl FnMut(&mut [f32], usize, std::time::Instant) + Send + 'static,
    on_error: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, DeviceError> {
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate.0;
    match format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                let timestamp = info.timestamp();
                let playback = std::time::Instant::now()
                    + timestamp
                        .playback
                        .duration_since(&timestamp.callback)
                        .unwrap_or_default();
                render(data, channels, playback)
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => {
            let mut scratch = vec![0.0; 2048 * channels];
            device.build_output_stream(
                config,
                move |data: &mut [i16], info: &cpal::OutputCallbackInfo| {
                    let timestamp = info.timestamp();
                    let playback = std::time::Instant::now()
                        + timestamp
                            .playback
                            .duration_since(&timestamp.callback)
                            .unwrap_or_default();
                    for (index, chunk) in data.chunks_mut(scratch.len()).enumerate() {
                        let offset = std::time::Duration::from_secs_f64(
                            (index * scratch.len() / channels) as f64 / sample_rate as f64,
                        );
                        render(&mut scratch[..chunk.len()], channels, playback + offset);
                        for (dst, src) in chunk.iter_mut().zip(&scratch) {
                            *dst = (src.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        }
                    }
                },
                on_error,
                None,
            )
        }
        _ => return Err(audio_error("unsupported sample format")),
    }
    .map_err(audio_error)
}

/// Rate conversion only on the headphone bus; no allocation or locking in either callback.
pub(crate) struct CueWriter {
    producer: rtrb::Producer<[f32; 2]>,
    previous: [f32; 2],
    phase: f64,
    step: f64,
}

impl CueWriter {
    pub(crate) fn new(producer: rtrb::Producer<[f32; 2]>, master_rate: u32, cue_rate: u32) -> Self {
        Self {
            producer,
            previous: [0.0; 2],
            phase: 0.0,
            step: master_rate as f64 / cue_rate as f64,
        }
    }

    pub fn push(&mut self, sample: [f32; 2]) {
        while self.phase < 1.0 {
            let value = std::array::from_fn(|i| {
                self.previous[i] + (sample[i] - self.previous[i]) * self.phase as f32
            });
            let _ = self.producer.push(value);
            self.phase += self.step;
        }
        self.phase -= 1.0;
        self.previous = sample;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_rate_conversion_has_expected_frame_count_and_bounded_samples() {
        for (master, cue) in [(48_000, 44_100), (44_100, 48_000), (96_000, 48_000)] {
            let (producer, mut consumer) = rtrb::RingBuffer::new(100_000);
            let mut writer = CueWriter::new(producer, master, cue);
            for _ in 0..master {
                writer.push([0.5, -0.5]);
            }
            assert!((consumer.slots() as i64 - cue as i64).abs() <= 1);
            while let Ok(sample) = consumer.pop() {
                assert!((0.0..=0.5).contains(&sample[0]));
                assert!((-0.5..=0.0).contains(&sample[1]));
            }
        }
    }
}
