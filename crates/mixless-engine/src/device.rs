use std::sync::atomic::Ordering;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::engine::{AudioRt, Shared};

#[cfg(target_os = "macos")]
mod macos;

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
    /// None uses the selected device's native sample rate.
    pub sample_rate: Option<u32>,
    /// None preserves the physical output depth. Independent of the float DSP stream.
    pub bit_depth: Option<u16>,
    /// None requests a device-supported low-latency buffer on macOS (256 frames
    /// where supported), or the device default on other platforms. An explicit
    /// size applies to both master and headphone streams and must be supported.
    pub buffer_frames: Option<u32>,
}

impl AudioConfig {
    pub const SAMPLE_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

    pub fn supports_sample_rate(rate: u32) -> bool {
        (8_000..=192_000).contains(&rate)
    }

    pub fn automatic_buffer_label() -> &'static str {
        if cfg!(target_os = "macos") {
            "Low latency (auto · 256 frames)"
        } else {
            "Device default"
        }
    }
}

#[derive(Clone, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub sample_rates: Vec<u32>,
    pub current_sample_rate: u32,
    pub output_formats: Vec<AudioOutputFormat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioOutputFormat {
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub is_float: bool,
}

impl AudioOutputFormat {
    pub fn label(self) -> String {
        format!(
            "{}-bit {}",
            self.bit_depth,
            if self.is_float { "float" } else { "PCM" }
        )
    }
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
            let sample_rates = AudioConfig::SAMPLE_RATES
                .into_iter()
                .filter(|rate| {
                    configs.iter().any(|c| {
                        supported_format(c.sample_format())
                            && c.min_sample_rate().0 <= *rate
                            && c.max_sample_rate().0 >= *rate
                    })
                })
                .collect();
            let current_sample_rate = device
                .default_output_config()
                .map_err(audio_error)?
                .sample_rate()
                .0;
            #[cfg(target_os = "macos")]
            let hardware = macos::HardwareOutput::query(Some(&name));
            #[cfg(target_os = "macos")]
            let current_sample_rate = hardware
                .as_ref()
                .map_or(current_sample_rate, |hardware| hardware.sample_rate());
            #[cfg(target_os = "macos")]
            let output_formats = hardware
                .map(|hardware| hardware.available_formats())
                .unwrap_or_default();
            #[cfg(not(target_os = "macos"))]
            let output_formats = configs
                .iter()
                .filter(|c| supported_format(c.sample_format()))
                .flat_map(|c| {
                    sample_rates
                        .iter()
                        .copied()
                        .filter(move |rate| {
                            c.min_sample_rate().0 <= *rate && c.max_sample_rate().0 >= *rate
                        })
                        .map(move |sample_rate| AudioOutputFormat {
                            sample_rate,
                            bit_depth: (c.sample_format().sample_size() * 8) as u16,
                            is_float: c.sample_format().is_float(),
                        })
                })
                .collect();
            Ok(AudioDevice {
                name,
                sample_rates,
                current_sample_rate,
                output_formats,
            })
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
    bit_depth: Option<u16>,
) -> Result<(cpal::StreamConfig, cpal::SampleFormat), DeviceError> {
    let default = device.default_output_config().map_err(audio_error)?;
    let rate = rate.unwrap_or(default.sample_rate().0);
    if !AudioConfig::supports_sample_rate(rate) {
        return Err(audio_error(
            "sample rate must be between 8000 and 192000 Hz",
        ));
    }
    let supported = device
        .supported_output_configs()
        .map_err(audio_error)?
        .filter(|c| {
            supported_format(c.sample_format())
                && bit_depth.is_none_or(|bits| c.sample_format().sample_size() * 8 == bits as usize)
                && c.min_sample_rate().0 <= rate
                && c.max_sample_rate().0 >= rate
        })
        .min_by_key(|c| {
            (
                c.channels() != default.channels(),
                c.sample_format() != default.sample_format(),
            )
        })
        .ok_or_else(|| {
            audio_error(format!(
                "device does not support {rate} Hz with the selected output depth"
            ))
        })?;
    let buffer_size = select_buffer_size(buffer, supported.buffer_size())?;
    let format = supported.sample_format();
    let mut config = supported.with_sample_rate(cpal::SampleRate(rate)).config();
    config.buffer_size = buffer_size;
    Ok((config, format))
}

fn select_buffer_size(
    requested: Option<u32>,
    supported: &cpal::SupportedBufferSize,
) -> Result<cpal::BufferSize, DeviceError> {
    if let Some(frames) = requested {
        if frames == 0 || frames > 8192 {
            return Err(audio_error("invalid buffer size"));
        }
        if cfg!(target_os = "macos") && matches!(supported, cpal::SupportedBufferSize::Unknown) {
            return Err(audio_error(
                "device does not report an adjustable buffer size",
            ));
        }
        if let cpal::SupportedBufferSize::Range { min, max } = supported {
            if frames < *min || frames > *max {
                return Err(audio_error(format!(
                    "buffer size must be {min} to {max} frames"
                )));
            }
        }
        return Ok(cpal::BufferSize::Fixed(frames));
    }
    if cfg!(target_os = "macos") {
        if let cpal::SupportedBufferSize::Range { min, max } = supported {
            if *max == 0 || min > max {
                return Err(audio_error("device reported an invalid buffer size range"));
            }
            return Ok(cpal::BufferSize::Fixed(256_u32.clamp((*min).max(1), *max)));
        }
        // CPAL's CoreAudio backend cannot set a fixed size for an unknown range.
        // Keep output available; the snapshot reports the actual callback size.
    }
    Ok(cpal::BufferSize::Default)
}

pub(crate) struct OutputStreams {
    master: cpal::Stream,
    cue: Option<cpal::Stream>,
    enabled: Arc<std::sync::atomic::AtomicBool>,
    pub sample_rate: u32,
    pub device_name: String,
    pub cue_name: Option<String>,
    pub master_output: mixless_protocol::AudioOutputSnapshot,
    pub cue_output: Option<mixless_protocol::AudioOutputSnapshot>,
    // Dropped after streams: an uncommitted change restores physical formats.
    #[cfg(target_os = "macos")]
    hardware: Vec<macos::HardwareChange>,
}

impl OutputStreams {
    pub fn prepare(shared: Arc<Shared>, settings: &AudioConfig) -> Result<Self, DeviceError> {
        // CoreAudio starts streams while building them. Emit silence until the
        // host has published the new rate and verified the physical format.
        let enabled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let master_device = find_device(settings.master_device.as_deref())?;
        let device_name = master_device.name().map_err(audio_error)?;
        if settings.cue_device.as_ref() == Some(&device_name) {
            return Err(audio_error(
                "headphone output must be a different device from master",
            ));
        }
        #[cfg(target_os = "macos")]
        let master_hardware = macos::HardwareChange::prepare(
            settings.master_device.as_deref(),
            settings.sample_rate,
            settings.bit_depth,
        )?;
        #[cfg(target_os = "macos")]
        let rate = Some(master_hardware.sample_rate());
        #[cfg(not(target_os = "macos"))]
        let rate = settings.sample_rate;
        let client_depth = if cfg!(target_os = "macos") {
            None
        } else {
            settings.bit_depth
        };
        let (config, format) =
            stream_config(&master_device, rate, settings.buffer_frames, client_depth)?;
        let mut master_output = output_snapshot(&config, format);
        #[cfg(target_os = "macos")]
        {
            master_output.hardware_format = Some(master_hardware.description());
        }
        let mut cue_output = None;
        #[cfg(target_os = "macos")]
        let mut hardware = vec![master_hardware];
        let mut rt = AudioRt::new(config.sample_rate.0 as f32);
        let cue = if let Some(name) = &settings.cue_device {
            let device = find_device(Some(name))?;
            #[cfg(target_os = "macos")]
            let cue_hardware = macos::HardwareChange::prepare(Some(name), None, None)?;
            #[cfg(target_os = "macos")]
            let cue_rate = Some(cue_hardware.sample_rate());
            #[cfg(not(target_os = "macos"))]
            let cue_rate = None;
            let (cue_config, cue_format) =
                stream_config(&device, cue_rate, settings.buffer_frames, None)?;
            let mut status = output_snapshot(&cue_config, cue_format);
            #[cfg(target_os = "macos")]
            {
                status.hardware_format = Some(cue_hardware.description());
                hardware.push(cue_hardware);
            }
            cue_output = Some(status);
            let (producer, mut consumer) = rtrb::RingBuffer::<[f32; 2]>::new(8192);
            rt.cue_output = Some(CueWriter::new(
                producer,
                config.sample_rate.0,
                cue_config.sample_rate.0,
            ));
            let cue_shared = shared.clone();
            let cue_enabled = enabled.clone();
            Some(build_stream(
                &device,
                &cue_config,
                cue_format,
                move |data, channels, _playback| {
                    if !cue_enabled.load(Ordering::Acquire) {
                        data.fill(0.);
                        return;
                    }
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
        let master_enabled = enabled.clone();
        let master = build_stream(
            &master_device,
            &config,
            format,
            move |data, channels, playback| {
                if !master_enabled.load(Ordering::Acquire) {
                    data.fill(0.);
                    return;
                }
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
            enabled,
            sample_rate: config.sample_rate.0,
            device_name,
            cue_name: settings.cue_device.clone(),
            master_output,
            cue_output,
            #[cfg(target_os = "macos")]
            hardware,
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
        self.enabled.store(false, Ordering::Release);
        if let Err(error) = self.master.pause() {
            self.activate();
            return Err(audio_error(error));
        }
        if let Some(cue) = &self.cue {
            let _ = cue.pause();
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), DeviceError> {
        #[cfg(target_os = "macos")]
        for hardware in &self.hardware {
            hardware.verify()?;
        }
        Ok(())
    }

    pub fn commit(&mut self) {
        #[cfg(target_os = "macos")]
        for hardware in &mut self.hardware {
            hardware.commit();
        }
        self.activate();
    }

    pub fn activate(&self) {
        self.enabled.store(true, Ordering::Release);
    }

    pub fn needs_refresh(&self, settings: &AudioConfig) -> bool {
        #[cfg(target_os = "macos")]
        {
            !self.hardware[0].selected_device_matches(settings.master_device.as_deref())
                || self.verify().is_err()
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = settings;
            false
        }
    }
}

fn output_snapshot(
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
) -> mixless_protocol::AudioOutputSnapshot {
    mixless_protocol::AudioOutputSnapshot {
        sample_rate: config.sample_rate.0,
        hardware_format: None,
        stream_format: AudioOutputFormat {
            sample_rate: config.sample_rate.0,
            bit_depth: (format.sample_size() * 8) as u16,
            is_float: format.is_float(),
        }
        .label(),
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
    fn manual_buffer_size_is_validated_without_clamping() {
        let range = cpal::SupportedBufferSize::Range { min: 64, max: 1024 };
        for frames in [64, 128, 256, 512, 1024] {
            assert_eq!(
                select_buffer_size(Some(frames), &range).unwrap(),
                cpal::BufferSize::Fixed(frames)
            );
        }
        for frames in [0, 32, 2048, 8193] {
            assert!(select_buffer_size(Some(frames), &range).is_err());
        }
        let unknown = cpal::SupportedBufferSize::Unknown;
        if cfg!(target_os = "macos") {
            assert!(select_buffer_size(Some(128), &unknown).is_err());
        } else {
            assert_eq!(
                select_buffer_size(Some(128), &unknown).unwrap(),
                cpal::BufferSize::Fixed(128)
            );
        }
        assert!(select_buffer_size(Some(0), &unknown).is_err());
        assert!(select_buffer_size(Some(8193), &unknown).is_err());
    }

    #[test]
    fn automatic_buffer_respects_device_limits_and_platform_policy() {
        for (min, max, expected) in [(32, 2048, 256), (512, 4096, 512), (32, 128, 128)] {
            assert_eq!(
                select_buffer_size(None, &cpal::SupportedBufferSize::Range { min, max }).unwrap(),
                if cfg!(target_os = "macos") {
                    cpal::BufferSize::Fixed(expected)
                } else {
                    cpal::BufferSize::Default
                }
            );
        }
        assert_eq!(
            select_buffer_size(None, &cpal::SupportedBufferSize::Unknown).unwrap(),
            cpal::BufferSize::Default
        );
        if cfg!(target_os = "macos") {
            for (min, max) in [(512, 128), (0, 0)] {
                assert!(
                    select_buffer_size(None, &cpal::SupportedBufferSize::Range { min, max })
                        .is_err()
                );
            }
        }
    }

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
