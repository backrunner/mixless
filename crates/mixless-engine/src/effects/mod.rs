//! Preallocated audio inserts: lifecycle and parameters live here; DSP cores and
//! per-sample dispatch are private submodules. Nothing is allocated by processing.

mod capture;
mod delay;
mod pitch;
mod process;
mod spectral;
mod vocoder;

#[cfg(test)]
mod tests;

use crate::dsp::{SmoothValue, StateFilter};
use capture::Capture;
use delay::{DelayLine, Reverb};
pub(crate) use mixless_protocol::FxKind as EffectKind;
use pitch::PitchShift;
use spectral::Spectral;
use vocoder::Vocoder;

#[derive(Clone, Copy)]
pub(crate) struct EffectParams {
    pub kind: EffectKind,
    pub mix: f32,
    pub bypass: bool,
    pub feedback: f32,
    pub beats: f32,
    pub rate_hz: f32,
    pub bpm: f32,
    pub depth: f32,
    pub drive: f32,
    pub decay_seconds: f32,
    pub size: f32,
    pub damping: f32,
}

impl Default for EffectParams {
    fn default() -> Self {
        Self {
            kind: EffectKind::Echo,
            mix: 0.5,
            bypass: true,
            feedback: 0.35,
            beats: 0.5,
            rate_hz: 0.0,
            bpm: 120.0,
            depth: 0.5,
            drive: 0.5,
            decay_seconds: 1.6,
            size: 0.5,
            damping: 0.5,
        }
    }
}

pub(crate) struct Effect {
    sample_rate: f32,
    params: EffectParams,
    kind: EffectKind,
    mix: SmoothValue,
    feed: SmoothValue,
    feedback: SmoothValue,
    depth: SmoothValue,
    drive: SmoothValue,
    delay: DelayLine,
    delay_old: f32,
    delay_new: f32,
    delay_blend: SmoothValue,
    delay_progress: f32,
    reverb: Reverb,
    capture: Capture,
    pitch: PitchShift,
    spectral: Spectral,
    vocoder: Vocoder,
    combs: [DelayLine; 3],
    phase: f32,
    phase_step: f32,
    carrier_phase: f32,
    carrier_step: f32,
    gate: [SmoothValue; 2],
    allpass: [[f32; 6]; 2],
    phaser_feedback: [f32; 2],
    phaser_coeff: f32,
    sample_counter: usize,
    age: usize,
    armed: bool,
    held: [f32; 2],
    noise: u32,
    filter: [StateFilter; 2],
    filter2: [StateFilter; 2],
    dub_low: [StateFilter; 2],
    dub_high: [StateFilter; 2],
    crush_levels: f32,
    crush_hold: usize,
    envelope: f32,
    attack: f32,
    release: f32,
    dynamics_gain: f32,
    gate_open: bool,
    dc_input: [f32; 2],
    dc_output: [f32; 2],
    freeze: SmoothValue,
}
impl Effect {
    pub fn new(sr: f32) -> Self {
        Self {
            sample_rate: sr,
            params: EffectParams::default(),
            kind: EffectKind::Echo,
            mix: SmoothValue::new(0.0, sr, 0.005),
            feed: SmoothValue::new(0.0, sr, 0.005),
            feedback: SmoothValue::new(0.35, sr, 0.01),
            depth: SmoothValue::new(0.5, sr, 0.01),
            drive: SmoothValue::new(0.5, sr, 0.01),
            delay: DelayLine::new((sr * 12.0) as usize),
            delay_old: sr * 0.25,
            delay_new: sr * 0.25,
            delay_blend: SmoothValue::new(1.0, sr, 0.01),
            delay_progress: 1.0,
            reverb: Reverb::new(sr),
            capture: Capture::new(sr),
            pitch: PitchShift::new(sr),
            spectral: Spectral::new(),
            vocoder: Vocoder::new(sr),
            combs: std::array::from_fn(|_| DelayLine::new((sr * 0.1) as usize)),
            phase: 0.0,
            phase_step: 0.0,
            carrier_phase: 0.0,
            carrier_step: 110.0 / sr,
            gate: std::array::from_fn(|_| SmoothValue::new(1.0, sr, 0.002)),
            allpass: [[0.0; 6]; 2],
            phaser_feedback: [0.0; 2],
            phaser_coeff: 0.0,
            sample_counter: 0,
            age: 0,
            armed: false,
            held: [0.0; 2],
            noise: 0x12345678,
            filter: std::array::from_fn(|_| StateFilter::new()),
            filter2: std::array::from_fn(|_| StateFilter::new()),
            dub_low: std::array::from_fn(|_| StateFilter::new()),
            dub_high: std::array::from_fn(|_| StateFilter::new()),
            crush_levels: 1024.0,
            crush_hold: 8,
            envelope: 0.0,
            attack: 1.0 - (-1.0 / (sr * 0.003)).exp(),
            release: 1.0 - (-1.0 / (sr * 0.08)).exp(),
            dynamics_gain: 1.0,
            gate_open: false,
            dc_input: [0.0; 2],
            dc_output: [0.0; 2],
            freeze: SmoothValue::new(0.0, sr, 0.03),
        }
    }
    pub fn configure(&mut self, mut p: EffectParams) {
        p.mix = p.mix.clamp(0.0, 1.0);
        p.depth = p.depth.clamp(0.0, 1.0);
        p.drive = p.drive.clamp(0.0, 1.0);
        p.bpm = p.bpm.clamp(20.0, 400.0);
        p.beats = p.beats.clamp(0.0625, p.kind.max_beats());
        let enabled = !p.bypass && p.kind == self.kind && p.mix > 0.0;
        if enabled && !self.armed {
            self.age = 0;
            self.capture.reset();
            self.freeze.set(0.0);
        }
        self.armed = enabled;
        self.params = p;
        self.mix.set(if enabled { p.mix } else { 0.0 });
        self.feed.set(if enabled { 1.0 } else { 0.0 });
        self.feedback.set(p.feedback.clamp(0.0, 0.88));
        self.depth.set(p.depth);
        self.drive.set(p.drive);
        self.reverb.configure(
            p.decay_seconds,
            if p.kind == EffectKind::Space {
                0.65 + 0.35 * p.size
            } else {
                p.size
            },
            p.damping,
        );
        for f in &mut self.dub_low {
            f.set(self.sample_rate, 3500.0, 0.707);
        }
        for f in &mut self.dub_high {
            f.set(self.sample_rate, 180.0, 0.707);
        }
        self.pitch.configure(p.depth, p.size, self.sample_rate);
        self.vocoder.configure(p.drive, self.sample_rate);
        self.carrier_step = 20.0 * 100.0f32.powf(p.drive) / self.sample_rate;
        self.crush_levels = (1u32 << (4.0 + p.drive * 12.0).round() as u32) as f32;
        self.crush_hold = (1.0 + (1.0 - p.depth) * 15.0).round() as usize;
        let rate = if p.rate_hz > 0.0 {
            p.rate_hz
        } else {
            p.bpm / (60.0 * p.beats)
        };
        self.phase_step = rate.clamp(0.01, 120.0) / self.sample_rate;
        let samples =
            (self.sample_rate * 60.0 * p.beats / p.bpm).clamp(16.0, self.delay.max_delay());
        if (samples - self.delay_new).abs() > 0.5 && self.delay_progress >= 1.0 {
            self.delay_old = self.delay_new;
            self.delay_new = samples;
            self.delay_blend = SmoothValue::new(0.0, self.sample_rate, 0.01);
            self.delay_blend.set(1.0);
            self.delay_progress = 0.0;
        }
    }
    pub fn reset(&mut self) {
        self.delay.reset();
        self.reverb.reset();
        self.capture.reset();
        self.pitch.reset();
        self.spectral.reset();
        self.vocoder = Vocoder::new(self.sample_rate);
        for c in &mut self.combs {
            c.reset();
        }
        self.allpass = [[0.0; 6]; 2];
        self.phaser_feedback = [0.0; 2];
        self.phase = 0.0;
        self.carrier_phase = 0.0;
        self.held = [0.0; 2];
        self.noise = 0x12345678;
        self.sample_counter = 0;
        self.age = 0;
        self.armed = false;
        self.envelope = 0.0;
        self.dynamics_gain = 1.0;
        self.gate_open = false;
        self.dc_input = [0.0; 2];
        self.dc_output = [0.0; 2];
        self.filter = std::array::from_fn(|_| StateFilter::new());
        self.filter2 = std::array::from_fn(|_| StateFilter::new());
        self.dub_low = std::array::from_fn(|_| StateFilter::new());
        self.dub_high = std::array::from_fn(|_| StateFilter::new());
        self.freeze = SmoothValue::new(0.0, self.sample_rate, 0.03);
    }
}
