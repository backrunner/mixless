//! Shared offline fixtures for behavioral engine tests.

mod allocation;
mod cues;
mod fx;
mod output;
mod performance;
mod pitch;
mod recording;
mod stems;
mod transport;

use super::*;
use std::sync::Arc;

fn sine_buffer(sr: u32, hz: f32, secs: f32) -> Arc<AudioBuffer> {
    let frames = (sr as f32 * secs) as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let s = (2.0 * std::f32::consts::PI * hz * n as f32 / sr as f32).sin() * 0.5;
        samples.push(s);
        samples.push(s);
    }
    Arc::new(AudioBuffer {
        loudness: Default::default(),
        samples,
        frames: frames as u64,
        sample_rate: sr,
    })
}

pub(super) fn test_engine(sample_rate: u32) -> Engine {
    let engine = Engine::new(EngineConfig {
        offline: true,
        sample_rate,
        block_frames: 256,
    })
    .unwrap();
    for index in 0..2 {
        let slot = &engine.shared.decks[index];
        let buffer = sine_buffer(sample_rate, if index == 0 { 440.0 } else { 550.0 }, 4.0);
        slot.frames.store(buffer.frames, Ordering::Relaxed);
        slot.src_sr.store(sample_rate, Ordering::Relaxed);
        *slot.buffer.lock().unwrap() = Some(buffer);
    }
    engine
}

fn energy(samples: &[f32]) -> f32 {
    samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len().max(1) as f32
}

fn left_channel(engine: &Engine, frames: usize) -> Vec<f32> {
    let mut samples = Vec::with_capacity(frames);
    while samples.len() < frames {
        let count = (frames - samples.len()).min(256);
        let output = engine.render_offline(count);
        samples.extend(output.chunks_exact(2).map(|sample| sample[0]));
    }
    samples
}
