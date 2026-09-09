//! Band-bank vocoder with a band-limited saw carrier.

use crate::dsp::{flush_small, StateFilter};

pub(super) fn band(f: &mut StateFilter, x: f32) -> f32 {
    let (lo, hi) = f.process(x);
    x - lo - hi
}
pub(super) struct Vocoder {
    analysis: [[StateFilter; 12]; 2],
    carrier: [[StateFilter; 12]; 2],
    envelope: [[f32; 12]; 2],
    phase: f32,
    step: f32,
    attack: f32,
    release: f32,
}
impl Vocoder {
    pub(super) fn new(sr: f32) -> Self {
        let bank = || {
            std::array::from_fn(|_| {
                std::array::from_fn(|i| {
                    let mut f = StateFilter::new();
                    f.set(sr, 100.0 * 60.0f32.powf(i as f32 / 11.0), 2.5);
                    f
                })
            })
        };
        Self {
            analysis: bank(),
            carrier: bank(),
            envelope: [[0.0; 12]; 2],
            phase: 0.0,
            step: 110.0 / sr,
            attack: 1.0 - (-1.0 / (sr * 0.003)).exp(),
            release: 1.0 - (-1.0 / (sr * 0.05)).exp(),
        }
    }
    pub(super) fn configure(&mut self, drive: f32, sample_rate: f32) {
        self.step = 55.0 * 8.0f32.powf(drive) / sample_rate;
    }

    pub(super) fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        self.phase = (self.phase + self.step).fract();
        let t = self.phase;
        let dt = self.step;
        let blep = if t < dt {
            let x = t / dt;
            x + x - x * x - 1.0
        } else if t > 1.0 - dt {
            let x = (t - 1.0) / dt;
            x * x + x + x + 1.0
        } else {
            0.0
        };
        let saw = 2.0 * t - 1.0 - blep;
        std::array::from_fn(|ch| {
            let mut sum = 0.0;
            for i in 0..12 {
                let level = band(&mut self.analysis[ch][i], input[ch]).abs();
                let env = &mut self.envelope[ch][i];
                let k = if level > *env {
                    self.attack
                } else {
                    self.release
                };
                *env = flush_small(*env + k * (level - *env));
                sum += band(&mut self.carrier[ch][i], saw) * *env;
            }
            sum * 4.0
        })
    }
}
