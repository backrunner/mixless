use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;

use crate::engine::{AudioRt, Shared};

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("no output device")]
    NoDevice,
    #[error("cpal: {0}")]
    Cpal(String),
}

pub struct OutputStream {
    _stream: cpal::Stream,
    pub sample_rate: u32,
    pub block_hint: u32,
    pub device_name: String,
}

pub fn start_default_output(shared: Arc<Shared>) -> Result<OutputStream, DeviceError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(DeviceError::NoDevice)?;
    let name = device.name().unwrap_or_else(|_| "System Default".into());
    let supported = device
        .default_output_config()
        .map_err(|e| DeviceError::Cpal(e.to_string()))?;
    let sample_rate = supported.sample_rate().0;
    shared
        .sample_rate
        .store(sample_rate, std::sync::atomic::Ordering::Relaxed);
    let channels = supported.channels() as usize;
    let format = supported.sample_format();

    let mut preferred: cpal::StreamConfig = supported.config();
    preferred.buffer_size = cpal::BufferSize::Fixed(256);
    let fallback = {
        let mut c = preferred.clone();
        c.buffer_size = cpal::BufferSize::Default;
        c
    };

    let err_fn = |e| eprintln!("mixless output: {e}");
    let stream = match format {
        cpal::SampleFormat::F32 => build_f32(
            &device,
            &preferred,
            &fallback,
            Arc::clone(&shared),
            channels,
            err_fn,
        )?,
        cpal::SampleFormat::I16 => {
            build_i16(&device, &preferred, &fallback, shared, channels, err_fn)?
        }
        other => return Err(DeviceError::Cpal(format!("unsupported format {other:?}"))),
    };
    stream
        .play()
        .map_err(|e| DeviceError::Cpal(e.to_string()))?;

    Ok(OutputStream {
        _stream: stream,
        sample_rate,
        block_hint: 256,
        device_name: name,
    })
}

fn build_f32(
    device: &cpal::Device,
    preferred: &cpal::StreamConfig,
    fallback: &cpal::StreamConfig,
    shared: Arc<Shared>,
    channels: usize,
    err_fn: fn(cpal::StreamError),
) -> Result<cpal::Stream, DeviceError> {
    let shared_fb = Arc::clone(&shared);
    let ch = channels;
    let mut rt = AudioRt::new(preferred.sample_rate.0 as f32);
    device
        .build_output_stream(
            preferred,
            move |data: &mut [f32], _| {
                Shared::render_interleaved(shared.as_ref(), &mut rt, data, channels)
            },
            err_fn,
            None,
        )
        .or_else(|_| {
            let mut rt = AudioRt::new(fallback.sample_rate.0 as f32);
            device.build_output_stream(
                fallback,
                move |data: &mut [f32], _| {
                    Shared::render_interleaved(shared_fb.as_ref(), &mut rt, data, ch)
                },
                err_fn,
                None,
            )
        })
        .map_err(|e| DeviceError::Cpal(e.to_string()))
}

fn build_i16(
    device: &cpal::Device,
    preferred: &cpal::StreamConfig,
    fallback: &cpal::StreamConfig,
    shared: Arc<Shared>,
    channels: usize,
    err_fn: fn(cpal::StreamError),
) -> Result<cpal::Stream, DeviceError> {
    // Scratch is pre-sized for a 2048-frame stereo block; larger callbacks process in place slices.
    fn render_i16(
        shared: &Shared,
        rt: &mut AudioRt,
        data: &mut [i16],
        channels: usize,
        scratch: &mut [f32],
    ) {
        for chunk in data.chunks_mut(scratch.len()) {
            Shared::render_interleaved(shared, rt, &mut scratch[..chunk.len()], channels);
            for (output, sample) in chunk.iter_mut().zip(scratch.iter()) {
                *output = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            }
        }
    }

    let mut scratch_a = vec![0.0f32; 2048 * channels.max(1)];
    let shared_a = Arc::clone(&shared);
    let ch = channels;
    let mut rt = AudioRt::new(preferred.sample_rate.0 as f32);
    device
        .build_output_stream(
            preferred,
            move |data: &mut [i16], _| {
                render_i16(shared_a.as_ref(), &mut rt, data, channels, &mut scratch_a)
            },
            err_fn,
            None,
        )
        .or_else(|_| {
            let mut rt = AudioRt::new(fallback.sample_rate.0 as f32);
            let mut scratch_b = vec![0.0f32; 2048 * ch.max(1)];
            let shared_b = Arc::clone(&shared);
            device.build_output_stream(
                fallback,
                move |data: &mut [i16], _| {
                    render_i16(shared_b.as_ref(), &mut rt, data, ch, &mut scratch_b)
                },
                err_fn,
                None,
            )
        })
        .map_err(|e| DeviceError::Cpal(e.to_string()))
}
