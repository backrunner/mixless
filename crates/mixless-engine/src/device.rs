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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub master_device: Option<String>,
    pub master_uid: Option<String>,
    pub cue_device: Option<String>,
    pub cue_uid: Option<String>,
    /// Zero-based output channels; device names remain backward compatible.
    pub master_channels: [u16; 2],
    pub cue_channels: [u16; 2],
    /// None uses the selected device's native sample rate.
    pub sample_rate: Option<u32>,
    /// None preserves the physical output depth. Independent of the float DSP stream.
    pub bit_depth: Option<u16>,
    /// None requests a device-supported low-latency buffer on macOS (256 frames
    /// where supported), or the device default on other platforms. An explicit
    /// size applies to both master and headphone streams and must be supported.
    pub buffer_frames: Option<u32>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            master_device: None,
            master_uid: None,
            cue_device: None,
            cue_uid: None,
            master_channels: [0, 1],
            cue_channels: [0, 1],
            sample_rate: None,
            bit_depth: None,
            buffer_frames: None,
        }
    }
}

pub fn validate_channel_pairs(
    master: [u16; 2],
    cue: Option<[u16; 2]>,
    channels: u16,
) -> Result<(), DeviceError> {
    if !(channels == 1 && master == [0, 1] && cue.is_none())
        && (master[0] == master[1] || master.iter().any(|c| *c >= channels))
    {
        return Err(audio_error("master channel pair is unavailable"));
    }
    if let Some(cue) = cue {
        if cue[0] == cue[1] || cue.iter().any(|c| *c >= channels || master.contains(c)) {
            return Err(audio_error(
                "headphone channels must be available and separate from master",
            ));
        }
    }
    Ok(())
}

impl AudioConfig {
    pub const SAMPLE_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

    pub fn supports_sample_rate(rate: u32) -> bool {
        (8_000..=192_000).contains(&rate)
    }

    pub fn automatic_buffer_label() -> &'static str {
        if cfg!(target_os = "macos") {
            "Automatic (256 frames)"
        } else {
            "Device default"
        }
    }
}

#[derive(Clone, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub uid: Option<String>,
    pub output_channels: u16,
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
            #[cfg(target_os = "macos")]
            let uid = macos::uid(&name).ok();
            #[cfg(not(target_os = "macos"))]
            let uid = None;
            Ok(AudioDevice {
                uid,
                output_channels: configs.iter().map(|c| c.channels()).max().unwrap_or(0),
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
    required_channels: u16,
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
                && (c.channels() >= required_channels
                    || c.channels() == 1 && required_channels == 2)
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
        let mut resolved = settings.clone();
        #[cfg(target_os = "macos")]
        {
            if let Some(uid) = &settings.master_uid {
                resolved.master_device = Some(macos::name_for_uid(uid)?);
            }
            if settings.cue_device.is_some() {
                if let Some(uid) = &settings.cue_uid {
                    resolved.cue_device = Some(macos::name_for_uid(uid)?);
                }
            }
        }
        let settings = &resolved;
        let master_device = find_device(settings.master_device.as_deref())?;
        let device_name = master_device.name().map_err(audio_error)?;
        let shared_device = settings.cue_device.as_ref() == Some(&device_name);
        let master_required = settings
            .master_channels
            .iter()
            .max()
            .copied()
            .unwrap_or(1)
            .saturating_add(1);
        let cue_required = settings
            .cue_channels
            .iter()
            .max()
            .copied()
            .unwrap_or(1)
            .saturating_add(1);
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
        let (config, format) = stream_config(
            &master_device,
            rate,
            settings.buffer_frames,
            client_depth,
            if shared_device {
                master_required.max(cue_required)
            } else {
                master_required
            },
        )?;
        let mut master_output = output_snapshot(&config, format);
        #[cfg(target_os = "macos")]
        {
            master_output.hardware_format = Some(master_hardware.description());
        }
        validate_channel_pairs(
            settings.master_channels,
            shared_device.then_some(settings.cue_channels),
            config.channels,
        )?;
        let mut cue_output = None;
        let mut shared_cue = None;
        let master_pair = settings.master_channels;
        let cue_pair = settings.cue_channels;
        #[cfg(target_os = "macos")]
        let mut hardware = vec![master_hardware];
        let mut rt = AudioRt::new(config.sample_rate.0 as f32);
        let cue = if shared_device {
            let (producer, consumer) = rtrb::RingBuffer::new(2048);
            rt.cue_output = Some(CueWriter::new(
                producer,
                config.sample_rate.0,
                config.sample_rate.0,
            ));
            shared_cue = Some(consumer);
            cue_output = Some(master_output.clone());
            None
        } else if let Some(name) = &settings.cue_device {
            let device = find_device(Some(name))?;
            #[cfg(target_os = "macos")]
            let cue_hardware = macos::HardwareChange::prepare(Some(name), None, None)?;
            #[cfg(target_os = "macos")]
            let cue_rate = Some(cue_hardware.sample_rate());
            #[cfg(not(target_os = "macos"))]
            let cue_rate = None;
            let (cue_config, cue_format) = stream_config(
                &device,
                cue_rate,
                settings.buffer_frames,
                None,
                cue_required,
            )?;
            validate_channel_pairs(cue_pair, None, cue_config.channels)?;
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
                        route_pair(frame, consumer.pop().unwrap_or([0.0; 2]), cue_pair);
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
                Shared::render_interleaved(
                    &shared,
                    &mut rt,
                    data,
                    channels,
                    playback,
                    master_pair,
                    shared_cue.as_mut().map(|c| (cue_pair, c)),
                );
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
            let resolved = settings
                .master_uid
                .as_ref()
                .and_then(|uid| macos::name_for_uid(uid).ok());
            !self.hardware[0]
                .selected_device_matches(resolved.as_deref().or(settings.master_device.as_deref()))
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

pub(crate) fn route_pair(frame: &mut [f32], sample: [f32; 2], pair: [u16; 2]) {
    if frame.len() == 1 && pair == [0, 1] {
        frame[0] = (sample[0] + sample[1]) * 0.5;
        return;
    }
    frame.fill(0.0);
    for (channel, value) in pair.into_iter().zip(sample) {
        if let Some(out) = frame.get_mut(channel as usize) {
            *out = value;
        }
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
        if self.step == 1. {
            let _ = self.producer.push(sample);
            return;
        }
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
    fn channel_routes_validate_disjoint_buses_and_preserve_silence() {
        assert!(validate_channel_pairs([0, 1], Some([2, 3]), 4).is_ok());
        for cue in [[1, 2], [2, 2], [3, 4]] {
            assert!(validate_channel_pairs([0, 1], Some(cue), 4).is_err());
        }
        let mut frame = [9.; 6];
        route_pair(&mut frame, [0.25, -0.5], [4, 5]);
        assert_eq!(frame, [0., 0., 0., 0., 0.25, -0.5]);
        let mut mono = [0.];
        route_pair(&mut mono, [0.2, 0.6], [0, 1]);
        assert_eq!(mono, [0.4]);
        assert_eq!(AudioConfig::default().master_channels, [0, 1]);
    }
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
