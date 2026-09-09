//! Signalsmith insert pitch shifting and dual-window granular playback.

use super::delay::DelayLine;

pub(super) struct PitchShift {
    spectral: crate::stretch::InsertPitch,
    line: DelayLine,
    phase: f32,
    ratio: f32,
    span: f32,
}
impl PitchShift {
    pub(super) fn new(sr: f32) -> Self {
        Self {
            spectral: crate::stretch::InsertPitch::new(sr),
            line: DelayLine::new((sr * 0.16) as usize),
            phase: 0.0,
            ratio: 1.0,
            span: sr * 0.06,
        }
    }
    pub(super) fn reset(&mut self) {
        self.spectral.reset();
        self.line.reset();
        self.phase = 0.0;
    }
    pub(super) fn configure(&mut self, depth: f32, size: f32, sr: f32) {
        self.spectral.configure(depth * 24.0 - 12.0);
        self.ratio = 2.0f32.powf((depth * 24.0 - 12.0) / 12.0);
        self.span = sr * (0.025 + size * 0.08);
    }
    pub(super) fn process(&mut self, input: [f32; 2], grain: bool) -> [f32; 2] {
        if !grain {
            return self.spectral.process(input);
        }
        self.line.push(input);
        if (self.ratio - 1.0).abs() < 0.00001 && !grain {
            return input;
        }
        let second = (self.phase + 0.5).fract();
        let offset = if grain {
            self.span * 0.15 * (self.phase * std::f32::consts::TAU).sin().abs()
        } else {
            0.0
        };
        let a = self.line.read(2.0 + self.span * self.phase + offset);
        let b = self.line.read(2.0 + self.span * second + offset);
        let weight = 0.5 - 0.5 * (self.phase * std::f32::consts::TAU).cos();
        self.phase = (self.phase
            + (if grain && (self.ratio - 1.0).abs() < 0.001 {
                0.5
            } else {
                1.0 - self.ratio
            }) / self.span)
            .rem_euclid(1.0);
        std::array::from_fn(|ch| a[ch] * weight + b[ch] * (1.0 - weight))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_changes_frequency_without_changing_frame_count() {
        fn energy(samples: &[f32], hz: f32, sr: f32) -> f32 {
            let (mut a, mut b) = (0.0, 0.0);
            for (i, &x) in samples.iter().enumerate() {
                let angle = std::f32::consts::TAU * hz * i as f32 / sr;
                a += x * angle.cos();
                b += x * angle.sin();
            }
            (a * a + b * b) / samples.len() as f32
        }
        for sr in [44100.0, 48000.0, 96000.0] {
            for (depth, target) in [(0.0, 220.0), (1.0, 880.0)] {
                let mut pitch = PitchShift::new(sr);
                pitch.configure(depth, 0.5, sr);
                let mut rendered = Vec::new();
                let mut stereo_error = 0.0;
                let mut stereo_energy = 0.0;
                for i in 0..sr as usize {
                    let x = (std::f32::consts::TAU * 440.0 * i as f32 / sr).sin() * 0.3;
                    let y = pitch.process([x, -x], false);
                    if i > sr as usize / 4 {
                        rendered.push(y[0]);
                        stereo_error += (y[0] + y[1]).powi(2);
                        stereo_energy += y[0] * y[0] + y[1] * y[1];
                    }
                }
                assert!(
                    stereo_error < stereo_energy * 0.01,
                    "stereo error {stereo_error} / {stereo_energy}"
                );
                let shifted = energy(&rendered, target, sr);
                let original = energy(&rendered, 440.0, sr);
                assert!(
                    shifted > original * 10.0 && shifted > 1.0,
                    "{sr} {target}: {shifted} vs {original}"
                );
            }
        }
    }
}
