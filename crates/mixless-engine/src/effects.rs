use crate::dsp::{flush_small, SmoothValue};

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(u32)]
pub(crate) enum EffectKind {
    Echo,
    Flanger,
    Gate,
    Reverb,
    Phaser,
}

impl EffectKind {
    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "echo" => Some(Self::Echo),
            "flanger" => Some(Self::Flanger),
            "gate" => Some(Self::Gate),
            "reverb" => Some(Self::Reverb),
            "phaser" => Some(Self::Phaser),
            _ => None,
        }
    }

    pub fn from_id(id: u32) -> Self {
        match id {
            1 => Self::Flanger,
            2 => Self::Gate,
            3 => Self::Reverb,
            4 => Self::Phaser,
            _ => Self::Echo,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EffectParams {
    pub kind: EffectKind,
    pub mix: f32,
    pub bypass: bool,
    pub feedback: f32,
    pub beats: f32,
    pub rate_hz: f32,
    pub bpm: f32,
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
        }
    }
}

struct DelayLine {
    samples: Vec<[f32; 2]>,
    stamps: Vec<u64>,
    generation: u64,
    write: usize,
}

impl DelayLine {
    fn new(frames: usize) -> Self {
        Self {
            samples: vec![[0.0; 2]; frames + 4],
            stamps: vec![0; frames + 4],
            generation: 1,
            write: 0,
        }
    }

    fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.write = 0;
    }

    fn read(&self, delay: f32) -> [f32; 2] {
        let delay = delay.clamp(1.0, (self.samples.len() - 2) as f32);
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

    fn push(&mut self, sample: [f32; 2]) {
        self.samples[self.write] = sample;
        self.stamps[self.write] = self.generation;
        self.write = (self.write + 1) % self.samples.len();
    }
}

struct Reverb {
    lines: [DelayLine; 4],
    lengths: [f32; 4],
    damping: [f32; 4],
    damping_gain: f32,
}

impl Reverb {
    fn new(sample_rate: f32) -> Self {
        let lengths =
            [0.0297, 0.0371, 0.0411, 0.0437].map(|seconds| (sample_rate * seconds).round());
        Self {
            lines: std::array::from_fn(|index| DelayLine::new(lengths[index] as usize)),
            lengths,
            damping: [0.0; 4],
            damping_gain: 1.0 - (-2.0 * std::f32::consts::PI * 5000.0 / sample_rate).exp(),
        }
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.reset();
        }
        self.damping = [0.0; 4];
    }

    fn process(&mut self, input: [f32; 2], feedback: f32) -> [f32; 2] {
        let taps: [f32; 4] =
            std::array::from_fn(|index| self.lines[index].read(self.lengths[index])[0]);
        for (index, tap) in taps.iter().enumerate() {
            self.damping[index] =
                flush_small(self.damping[index] + self.damping_gain * (tap - self.damping[index]));
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
            self.lines[index].push([
                flush_small(injection[index] * 0.5 + matrix[index] * 0.5 * feedback),
                0.0,
            ]);
        }
        [(taps[0] + taps[2]) * 0.5, (taps[1] + taps[3]) * 0.5]
    }
}

pub(crate) struct Effect {
    sample_rate: f32,
    params: EffectParams,
    kind: EffectKind,
    mix: SmoothValue,
    feed: SmoothValue,
    feedback: SmoothValue,
    delay: DelayLine,
    delay_old: f32,
    delay_new: f32,
    delay_blend: SmoothValue,
    delay_progress: f32,
    reverb: Reverb,
    phase: f32,
    phase_step: f32,
    gate: SmoothValue,
    allpass: [[f32; 6]; 2],
    phaser_feedback: [f32; 2],
    phaser_coeff: f32,
    counter: usize,
}

impl Effect {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            params: EffectParams::default(),
            kind: EffectKind::Echo,
            mix: SmoothValue::new(0.0, sample_rate, 0.005),
            feed: SmoothValue::new(0.0, sample_rate, 0.005),
            feedback: SmoothValue::new(0.35, sample_rate, 0.01),
            delay: DelayLine::new((sample_rate * 4.0) as usize),
            delay_old: sample_rate * 0.25,
            delay_new: sample_rate * 0.25,
            delay_blend: SmoothValue::new(1.0, sample_rate, 0.01),
            delay_progress: 1.0,
            reverb: Reverb::new(sample_rate),
            phase: 0.0,
            phase_step: 0.0,
            gate: SmoothValue::new(1.0, sample_rate, 0.002),
            allpass: [[0.0; 6]; 2],
            phaser_feedback: [0.0; 2],
            phaser_coeff: 0.0,
            counter: 0,
        }
    }

    pub fn configure(&mut self, params: EffectParams) {
        self.params = params;
        let enabled = !params.bypass && params.kind == self.kind;
        self.mix.set(if enabled { params.mix } else { 0.0 });
        self.feed.set(if enabled && params.mix > 0.0 {
            1.0
        } else {
            0.0
        });
        self.feedback.set(params.feedback.clamp(0.0, 0.88));
        let rate = if params.rate_hz > 0.0 {
            params.rate_hz
        } else {
            match params.kind {
                EffectKind::Gate => params.bpm / (60.0 * params.beats.max(0.0625)),
                _ => params.bpm / (60.0 * 8.0),
            }
        };
        self.phase_step = rate.clamp(0.01, 30.0) / self.sample_rate;
        let samples = (self.sample_rate * 60.0 * params.beats / params.bpm.max(20.0))
            .clamp(1.0, (self.delay.samples.len() - 2) as f32);
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
        self.allpass = [[0.0; 6]; 2];
        self.phaser_feedback = [0.0; 2];
        self.phase = 0.0;
    }

    pub fn process(&mut self, input: [f32; 2], send: bool) -> [f32; 2] {
        let mix = self.mix.next();
        let feed = self.feed.next();
        let feedback = self.feedback.next();
        if mix == 0.0 && self.kind != self.params.kind {
            self.kind = self.params.kind;
            self.reset();
            self.configure(self.params);
        }
        self.phase = (self.phase + self.phase_step).fract();
        let wet = match self.kind {
            EffectKind::Echo => {
                self.delay_progress = self.delay_blend.next();
                let before = self.delay.read(self.delay_old);
                let after = self.delay.read(self.delay_new);
                let delayed: [f32; 2] = std::array::from_fn(|channel| {
                    before[channel] + self.delay_progress * (after[channel] - before[channel])
                });
                self.delay.push(std::array::from_fn(|channel| {
                    flush_small(input[channel] * feed + delayed[channel] * feedback)
                }));
                delayed
            }
            EffectKind::Reverb => self
                .reverb
                .process(input.map(|sample| sample * feed), 0.70 + feedback * 0.27),
            EffectKind::Flanger => {
                if mix == 0.0 && feed == 0.0 {
                    return if send { [0.0; 2] } else { input };
                }
                let modulation = (self.phase * std::f32::consts::TAU).sin();
                let left = self
                    .delay
                    .read(self.sample_rate * (0.00275 + 0.00225 * modulation))[0];
                let right = self
                    .delay
                    .read(self.sample_rate * (0.00275 - 0.00225 * modulation))[1];
                let delayed = [left, right];
                self.delay.push(std::array::from_fn(|channel| {
                    flush_small(input[channel] * feed + delayed[channel] * feedback * 0.7)
                }));
                std::array::from_fn(|channel| (input[channel] + delayed[channel]) * 0.5)
            }
            EffectKind::Gate => {
                self.gate.set(if self.phase < 0.5 { 1.0 } else { 0.0 });
                let gain = self.gate.next();
                input.map(|sample| sample * gain)
            }
            EffectKind::Phaser => {
                if mix == 0.0 && feed == 0.0 {
                    return if send { [0.0; 2] } else { input };
                }
                if self.counter == 0 {
                    let modulation = ((self.phase * std::f32::consts::TAU).sin() + 1.0) * 0.5;
                    let frequency = 250.0 * 12.0f32.powf(modulation);
                    let tangent = (std::f32::consts::PI * frequency / self.sample_rate).tan();
                    self.phaser_coeff = (tangent - 1.0) / (tangent + 1.0);
                }
                self.counter = (self.counter + 1) % 16;
                std::array::from_fn(|channel| {
                    let mut sample =
                        input[channel] + self.phaser_feedback[channel] * feedback * 0.5;
                    for state in &mut self.allpass[channel] {
                        let output = self.phaser_coeff * sample + *state;
                        *state = flush_small(sample - self.phaser_coeff * output);
                        sample = output;
                    }
                    self.phaser_feedback[channel] = sample;
                    (input[channel] + sample) * 0.5
                })
            }
        };
        std::array::from_fn(|channel| {
            if send {
                wet[channel] * mix
            } else {
                input[channel] + mix * (wet[channel] - input[channel])
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_is_dry_at_zero_mix_and_when_bypassed() {
        for kind in [
            EffectKind::Echo,
            EffectKind::Flanger,
            EffectKind::Gate,
            EffectKind::Reverb,
            EffectKind::Phaser,
        ] {
            for bypass in [true, false] {
                let mut effect = Effect::new(48_000.0);
                effect.configure(EffectParams {
                    kind,
                    mix: 0.0,
                    bypass,
                    ..EffectParams::default()
                });
                for frame in 0..1200 {
                    let input = [(frame as f32 * 0.17).sin() * 0.2, -0.1];
                    assert_eq!(effect.process(input, false), input, "{kind:?}");
                }
            }
        }
    }

    #[test]
    fn echo_is_tempo_synced_and_send_is_wet_only() {
        for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
            let mut effect = Effect::new(sample_rate);
            effect.configure(EffectParams {
                mix: 1.0,
                bypass: false,
                feedback: 0.5,
                ..EffectParams::default()
            });
            for _ in 0..2000 {
                effect.process([0.0; 2], true);
            }
            let delay = (sample_rate * 0.25).round() as usize;
            assert_eq!(effect.process([0.5, 0.0], true), [0.0; 2]);
            for _ in 1..delay {
                assert_eq!(effect.process([0.0; 2], true), [0.0; 2]);
            }
            assert!((effect.process([0.0; 2], true)[0] - 0.5).abs() < 0.001);
            for _ in 1..delay {
                effect.process([0.0; 2], true);
            }
            assert!((effect.process([0.0; 2], true)[0] - 0.25).abs() < 0.001);
        }
    }

    #[test]
    fn effects_have_distinct_audible_output_and_bounded_release() {
        for kind in [
            EffectKind::Echo,
            EffectKind::Flanger,
            EffectKind::Gate,
            EffectKind::Reverb,
            EffectKind::Phaser,
        ] {
            let mut effect = Effect::new(48_000.0);
            effect.configure(EffectParams {
                kind,
                mix: 0.7,
                bypass: false,
                ..EffectParams::default()
            });
            let mut difference = 0.0;
            for frame in 0..48_000 {
                let input = [
                    (frame as f32 * 0.123).sin() * 0.1,
                    (frame as f32 * 0.19).sin() * 0.1,
                ];
                let output = effect.process(input, false);
                assert!(output
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() < 2.0));
                difference += (input[0] - output[0]).abs();
            }
            assert!(difference > 1.0, "{kind:?} appears bypassed");
            effect.configure(EffectParams {
                kind,
                bypass: true,
                ..EffectParams::default()
            });
            for _ in 0..1000 {
                effect.process([0.1, -0.1], false);
            }
            assert_eq!(effect.process([0.1, -0.1], false), [0.1, -0.1]);
        }
    }

    #[test]
    fn reverb_tail_decays_without_input() {
        let mut effect = Effect::new(48_000.0);
        effect.configure(EffectParams {
            kind: EffectKind::Reverb,
            mix: 1.0,
            bypass: false,
            ..EffectParams::default()
        });
        for _ in 0..1000 {
            effect.process([0.0; 2], true);
        }
        effect.process([1.0, 0.0], true);
        let mut early = 0.0;
        let mut late = 0.0;
        for frame in 0..144_000 {
            let output = effect.process([0.0; 2], true);
            let energy = output[0] * output[0] + output[1] * output[1];
            if frame < 48_000 {
                early += energy;
            }
            if frame >= 96_000 {
                late += energy;
            }
        }
        assert!(early > 0.01 && late < early * 0.01, "{early} -> {late}");
    }
}
