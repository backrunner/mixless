//! Fractional delay storage and feedback delay network reverb.

use crate::dsp::{flush_small, SmoothValue};

pub(super) struct DelayLine {
    samples: Vec<[f32; 2]>,
    stamps: Vec<u64>,
    generation: u64,
    write: usize,
}

impl DelayLine {
    pub(super) fn new(frames: usize) -> Self {
        Self {
            samples: vec![[0.0; 2]; frames + 4],
            stamps: vec![0; frames + 4],
            generation: 1,
            write: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.write = 0;
    }

    pub(super) fn max_delay(&self) -> f32 {
        (self.samples.len() - 2) as f32
    }

    pub(super) fn read(&self, delay: f32) -> [f32; 2] {
        let delay = delay.clamp(1.0, self.max_delay());
        let whole = delay as usize;
        let fraction = delay - whole as f32;
        let first = (self.write + self.samples.len() - whole) % self.samples.len();
        let second = (first + self.samples.len() - 1) % self.samples.len();
        let before = if self.stamps[first] == self.generation {
            self.samples[first]
        } else {
            [0.0; 2]
        };
        let after = if self.stamps[second] == self.generation {
            self.samples[second]
        } else {
            [0.0; 2]
        };
        std::array::from_fn(|channel| {
            before[channel] + fraction * (after[channel] - before[channel])
        })
    }

    pub(super) fn push(&mut self, sample: [f32; 2]) {
        self.samples[self.write] = sample;
        self.stamps[self.write] = self.generation;
        self.write = (self.write + 1) % self.samples.len();
    }
}

pub(super) struct Reverb {
    lines: [DelayLine; 4],
    lengths: [f32; 4],
    damping: [f32; 4],
    damping_gain: SmoothValue,
    size: SmoothValue,
    gains: [SmoothValue; 4],
    sample_rate: f32,
}

impl Reverb {
    pub(super) fn new(sample_rate: f32) -> Self {
        let lengths =
            [0.0297, 0.0371, 0.0411, 0.0437].map(|seconds| (sample_rate * seconds).round());
        Self {
            lines: std::array::from_fn(|index| DelayLine::new((lengths[index] * 2.0) as usize)),
            lengths,
            damping: [0.0; 4],
            damping_gain: SmoothValue::new(0.5, sample_rate, 0.02),
            size: SmoothValue::new(1.0, sample_rate, 0.04),
            gains: std::array::from_fn(|_| SmoothValue::new(0.8, sample_rate, 0.02)),
            sample_rate,
        }
    }

    pub(super) fn configure(&mut self, decay: f32, size: f32, damping: f32) {
        let scale = 0.5 + size.clamp(0.0, 1.0) * 1.5;
        self.size.set(scale);
        let cutoff =
            (16_000.0 * 0.05f32.powf(damping.clamp(0.0, 1.0))).min(self.sample_rate * 0.45);
        self.damping_gain
            .set(1.0 - (-std::f32::consts::TAU * cutoff / self.sample_rate).exp());
        for (gain, length) in self.gains.iter_mut().zip(self.lengths) {
            gain.set(
                10.0f32.powf(-3.0 * length * scale / (self.sample_rate * decay.clamp(0.2, 8.0))),
            );
        }
    }

    pub(super) fn reset(&mut self) {
        for line in &mut self.lines {
            line.reset();
        }
        self.damping = [0.0; 4];
    }

    pub(super) fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        self.process_frozen(input, 0.0)
    }

    pub(super) fn process_frozen(&mut self, input: [f32; 2], freeze: f32) -> [f32; 2] {
        let size = self.size.next();
        let damping_gain = self.damping_gain.next() * (1.0 - freeze) + freeze;
        let taps: [f32; 4] =
            std::array::from_fn(|index| self.lines[index].read(self.lengths[index] * size)[0]);
        for (index, tap) in taps.iter().enumerate() {
            self.damping[index] =
                flush_small(self.damping[index] + damping_gain * (tap - self.damping[index]));
        }
        let [first, second, third, fourth] = self.damping;
        let matrix = [
            first + second + third + fourth,
            first - second + third - fourth,
            first + second - third - fourth,
            first - second - third + fourth,
        ];
        let injection = [input[0], input[1], -input[1], input[0]];
        for index in 0..4 {
            let gain = self.gains[index].next();
            let feedback = gain + freeze * (0.99999 - gain);
            self.lines[index].push([
                flush_small(injection[index] * 0.5 + matrix[index] * 0.5 * feedback),
                0.0,
            ]);
        }
        [(taps[0] + taps[2]) * 0.5, (taps[1] + taps[3]) * 0.5]
    }
}
