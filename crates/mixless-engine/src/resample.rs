#[cfg(test)]
use crate::decode::AudioBuffer;
use crate::source::SampleSource;

const TAPS: usize = 48;
const PHASES: usize = 128;
const RATE_STEPS: usize = 21;

pub(crate) struct Resampler {
    kernels: Vec<f32>,
}

impl Resampler {
    pub fn new() -> Self {
        let mut kernels = Vec::with_capacity(RATE_STEPS * (PHASES + 1) * TAPS);
        for rate_index in 0..RATE_STEPS {
            let rate = 2.0f64.powf(rate_index as f64 / 4.0);
            let cutoff = 0.90 / rate;
            for phase in 0..=PHASES {
                let fraction = phase as f64 / PHASES as f64;
                let start = kernels.len();
                let mut total = 0.0;
                for tap in 0..TAPS {
                    let distance = tap as f64 - (TAPS / 2 - 1) as f64 - fraction;
                    let argument = std::f64::consts::PI * distance * cutoff;
                    let sinc = if argument.abs() < 1e-10 {
                        cutoff
                    } else {
                        argument.sin() / (std::f64::consts::PI * distance)
                    };
                    let window_phase = std::f64::consts::TAU * tap as f64 / (TAPS - 1) as f64;
                    let window =
                        0.42 - 0.5 * window_phase.cos() + 0.08 * (2.0 * window_phase).cos();
                    let weight = (sinc * window) as f32;
                    total += weight;
                    kernels.push(weight);
                }
                for weight in &mut kernels[start..] {
                    *weight /= total;
                }
            }
        }
        Self { kernels }
    }

    pub fn stereo_at(&self, buffer: &impl SampleSource, frame: f64, speed: f64) -> (f32, f32) {
        if !frame.is_finite()
            || frame < 0.0
            || frame >= buffer.frames() as f64
            || buffer.frames() == 0
        {
            return (0.0, 0.0);
        }
        if speed.abs() <= 1.0 {
            return buffer.stereo_at(frame);
        }
        let rate_position = (speed.abs().log2() * 4.0).clamp(0.0, (RATE_STEPS - 1) as f64);
        let rate_index = rate_position as usize;
        let next_rate = (rate_index + 1).min(RATE_STEPS - 1);
        let rate_fraction = (rate_position - rate_index as f64) as f32;
        let phase = frame.fract() * PHASES as f64;
        let phase_index = phase as usize;
        let fraction = (phase - phase_index as f64) as f32;
        let first = (rate_index * (PHASES + 1) + phase_index) * TAPS;
        let second = first + TAPS;
        let next_first = (next_rate * (PHASES + 1) + phase_index) * TAPS;
        let next_second = next_first + TAPS;
        let center = frame.floor() as i64;
        let mut left = 0.0;
        let mut right = 0.0;
        for tap in 0..TAPS {
            let index = (center + tap as i64 - (TAPS / 2 - 1) as i64)
                .clamp(0, buffer.frames() as i64 - 1) as usize;
            let lower = self.kernels[first + tap]
                + fraction * (self.kernels[second + tap] - self.kernels[first + tap]);
            let upper = self.kernels[next_first + tap]
                + fraction * (self.kernels[next_second + tap] - self.kernels[next_first + tap]);
            let weight = lower + rate_fraction * (upper - lower);
            let sample = buffer.sample(index);
            left += sample[0] * weight;
            right += sample[1] * weight;
        }
        let blend = ((speed.abs() - 1.0) * 20.0).min(1.0) as f32;
        if blend < 1.0 {
            let (dry_left, dry_right) = buffer.stereo_at(frame);
            (
                dry_left + blend * (left - dry_left),
                dry_right + blend * (right - dry_right),
            )
        } else {
            (left, right)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_scratching_rejects_above_nyquist_content() {
        let resampler = Resampler::new();
        let mut samples = Vec::new();
        for frame in 0..4800 {
            let sample = (std::f32::consts::TAU * 15000.0 * frame as f32 / 48000.0).sin();
            samples.extend_from_slice(&[sample, sample]);
        }
        let buffer = AudioBuffer {
            loudness: Default::default(),
            samples,
            frames: 4800,
            sample_rate: 48000,
        };
        for direction in [-2.0, 2.0] {
            let mut energy = 0.0;
            for frame in 0..1000 {
                let position = if direction > 0.0 {
                    100.0 + frame as f64 * 2.0
                } else {
                    4700.0 - frame as f64 * 2.0
                };
                let sample = resampler.stereo_at(&buffer, position, direction).0;
                energy += sample * sample;
            }
            assert!((energy / 1000.0).sqrt() < 0.01);
        }
    }

    #[test]
    fn resampler_preserves_dc_and_bounds() {
        let resampler = Resampler::new();
        let buffer = AudioBuffer {
            loudness: Default::default(),
            samples: vec![0.25; 2000],
            frames: 1000,
            sample_rate: 48000,
        };
        for rate in [1.01, 1.3, 2.0, 4.0, 8.0] {
            for frame in [0.0, 100.37, 998.5] {
                assert!((resampler.stereo_at(&buffer, frame, rate).0 - 0.25).abs() < 1e-5);
            }
        }
        assert_eq!(resampler.stereo_at(&buffer, -1.0, 2.0), (0.0, 0.0));
    }
}
