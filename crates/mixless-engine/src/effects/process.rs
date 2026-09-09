//! Sample dispatch, beat phase, bypass crossfades and wet-only send mixing.

use super::{vocoder::band, Effect, EffectKind};
use crate::dsp::flush_small;

impl Effect {
    pub fn process(&mut self, input: [f32; 2], send: bool) -> [f32; 2] {
        self.process_clocked(input, send, None)
    }
    fn noise_sample(&mut self) -> f32 {
        self.noise = self.noise.wrapping_mul(1664525).wrapping_add(1013904223);
        self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
    fn delay_tap(&mut self) -> [f32; 2] {
        self.delay_progress = self.delay_blend.next();
        let a = self.delay.read(self.delay_old);
        let b = self.delay.read(self.delay_new);
        std::array::from_fn(|ch| a[ch] + self.delay_progress * (b[ch] - a[ch]))
    }
    pub fn process_clocked(&mut self, input: [f32; 2], send: bool, beat: Option<f64>) -> [f32; 2] {
        use std::f32::consts::{PI, TAU};
        use EffectKind::*;
        let mix = self.mix.next();
        let feed = self.feed.next();
        let fb = self.feedback.next();
        let depth = self.depth.next();
        let drive = self.drive.next();
        if mix == 0.0 && self.kind != self.params.kind {
            self.kind = self.params.kind;
            self.reset();
            self.configure(self.params);
        }
        if mix == 0.0 && !self.kind.has_tail() {
            return if send { [0.0; 2] } else { input };
        }
        if self.params.rate_hz == 0.0 && self.kind.is_modulated() {
            if let Some(beat) = beat {
                self.phase = (beat / self.params.beats as f64).rem_euclid(1.0) as f32;
            }
        }
        self.sample_counter = self.sample_counter.wrapping_add(1);
        if self.armed {
            self.age = self.age.saturating_add(1);
        }
        let period = (self.sample_rate * 60.0 * self.params.beats / self.params.bpm).max(16.0);
        let t = self.age as f32 / period;
        let level = input[0].abs().max(input[1].abs());
        let k = if level > self.envelope {
            self.attack
        } else {
            self.release
        };
        self.envelope = flush_small(self.envelope + k * (level - self.envelope));
        self.carrier_phase = (self.carrier_phase + self.carrier_step).fract();
        let kind = self.kind;
        let wet = match kind {
            Echo | PingPong | DubEcho | Delay | EchoOut | SpaceEcho | LowCutEcho | TapeEcho
            | PatternDelay | PitchDelay | Spiral | ReverseDelay => {
                let mut delayed = self.delay_tap();
                if matches!(kind, SpaceEcho | PatternDelay) {
                    let a = self.delay.read(self.delay_new * 0.5);
                    let b = self.delay.read(self.delay_new * 0.75);
                    delayed =
                        std::array::from_fn(|ch| delayed[ch] * 0.5 + a[ch] * 0.2 + b[ch] * 0.3);
                }
                if matches!(kind, TapeEcho | SpaceEcho) {
                    let tap = self.delay.read(
                        self.delay_new + (self.phase * TAU).sin() * self.sample_rate * 0.0007,
                    );
                    delayed = std::array::from_fn(|ch| delayed[ch] * 0.5 + tap[ch] * 0.5);
                }
                if matches!(kind, PitchDelay | Spiral) {
                    delayed = self.pitch.process(delayed, false);
                }
                for ch in 0..2 {
                    if matches!(kind, DubEcho | TapeEcho | SpaceEcho) {
                        delayed[ch] = self.dub_low[ch].process(delayed[ch]).0;
                    }
                    if matches!(kind, DubEcho | LowCutEcho) {
                        delayed[ch] = self.dub_high[ch].process(delayed[ch]).1;
                    }
                    if matches!(kind, DubEcho | TapeEcho | SpaceEcho) {
                        delayed[ch] = (delayed[ch] * 1.3).tanh() / 1.3;
                    }
                }
                let injection = if kind == ReverseDelay {
                    self.capture
                        .process(input, period as usize, kind, depth, fb)
                } else if kind == PingPong {
                    [(input[0] + input[1]) * 0.5, 0.0]
                } else {
                    input
                };
                let injection_gain = if kind == EchoOut && t >= 1.0 {
                    0.0
                } else {
                    feed
                };
                let feedback = if kind == Delay { 0.0 } else { fb };
                self.delay.push(std::array::from_fn(|ch| {
                    flush_small(
                        injection[ch] * injection_gain
                            + delayed[if kind == PingPong { 1 - ch } else { ch }] * feedback,
                    )
                }));
                delayed
            }
            Roll | SlipRoll | LoopRoll | Beatmasher | Stutter | VinylBrake | TapeStop
            | Backspin | BeatLoop | OneShot | Reverse | Slicer | Censor | ReverseRoll | Helix => {
                self.capture
                    .process(input, period as usize, kind, depth, fb)
            }
            Reverb | Space | FreezeVerb | ReverbOut => {
                let frozen = kind == FreezeVerb && t >= 1.0 && self.armed;
                self.freeze.set(if frozen { 1.0 } else { 0.0 });
                let freeze = self.freeze.next();
                let injection = if kind == ReverbOut && t >= 1.0 {
                    0.0
                } else {
                    feed * (1.0 - freeze)
                };
                let mut wet = self
                    .reverb
                    .process_frozen(input.map(|x| x * injection), freeze);
                if kind == Space {
                    let early = self.delay.read(self.sample_rate * 0.07);
                    self.delay.push(input.map(|x| x * injection));
                    wet = std::array::from_fn(|ch| wet[ch] * 0.8 + early[ch] * 0.2);
                }
                wet
            }
            Flanger | Jet => {
                let m = if kind == Jet {
                    self.phase * 2.0 - 1.0
                } else {
                    (self.phase * TAU).sin()
                };
                let span = if kind == Jet { 0.006 } else { 0.00225 };
                let left = self
                    .delay
                    .read(self.sample_rate * (span + 0.0005 + span * m * depth))[0];
                let right = self
                    .delay
                    .read(self.sample_rate * (span + 0.0005 - span * m * depth))[1];
                let tap = [left, right];
                self.delay.push(std::array::from_fn(|ch| {
                    flush_small(input[ch] * feed + tap[ch] * fb * 0.7)
                }));
                std::array::from_fn(|ch| (input[ch] + tap[ch]) * 0.5)
            }
            Mobius | MobiusTriangle | Resonator | Robot => {
                let mut wet = [0.0; 2];
                for i in 0..3 {
                    let phase = (self.phase + i as f32 / 3.0).fract();
                    let weight = if kind == MobiusTriangle {
                        1.0 - (2.0 * phase - 1.0).abs()
                    } else {
                        0.5 - 0.5 * (TAU * phase).cos()
                    };
                    let freq = if matches!(kind, Mobius | MobiusTriangle) {
                        80.0 * 32.0f32.powf(phase)
                    } else {
                        60.0 * 16.0f32.powf(depth) * [1.0, 1.5, 2.0][i]
                    };
                    let tap = self.combs[i].read(self.sample_rate / freq);
                    self.combs[i].push(std::array::from_fn(|ch| {
                        flush_small(input[ch] * (1.0 - fb) + tap[ch] * fb)
                    }));
                    for ch in 0..2 {
                        wet[ch] += tap[ch]
                            * if matches!(kind, Mobius | MobiusTriangle) {
                                weight / 1.5
                            } else {
                                1.0 / 3.0
                            };
                    }
                }
                wet
            }
            Gate | Transform => std::array::from_fn(|ch| {
                let phase = (self.phase
                    + if kind == Transform {
                        ch as f32 * 0.5
                    } else {
                        0.0
                    })
                .fract();
                self.gate[ch].set(if phase < 0.1 + depth * 0.8 { 1.0 } else { 0.0 });
                input[ch] * self.gate[ch].next()
            }),
            Phaser => {
                if self.sample_counter % 16 == 0 {
                    let m = ((self.phase * TAU).sin() * depth + 1.0) * 0.5;
                    let freq = (250.0 * 12.0f32.powf(m)).min(self.sample_rate * 0.4);
                    let tangent = (PI * freq / self.sample_rate).tan();
                    self.phaser_coeff = (tangent - 1.0) / (tangent + 1.0);
                }
                std::array::from_fn(|ch| {
                    let mut x = input[ch] + self.phaser_feedback[ch] * fb * 0.5;
                    for state in &mut self.allpass[ch] {
                        let y = self.phaser_coeff * x + *state;
                        *state = flush_small(x - self.phaser_coeff * y);
                        x = y;
                    }
                    self.phaser_feedback[ch] = x;
                    (input[ch] + x) * 0.5
                })
            }
            Chorus => {
                let m = (self.phase * TAU).sin();
                let base = self.sample_rate * 0.018;
                let span = self.sample_rate * 0.01 * depth;
                let tap = [
                    self.delay.read(base + span * m)[0],
                    self.delay.read(base - span * m)[1],
                ];
                self.delay.push(input);
                tap
            }
            Tremolo | AutoPan | Lfo | AutoSidechain => {
                let m = if kind == Lfo {
                    1.0 - (2.0 * self.phase - 1.0).abs()
                } else if kind == AutoSidechain {
                    (-self.phase * 7.0).exp()
                } else {
                    0.5 + 0.5 * (self.phase * TAU).sin()
                };
                let l = 1.0 - depth * m;
                let r = if kind == AutoPan {
                    1.0 - depth * (1.0 - m)
                } else {
                    l
                };
                [input[0] * l, input[1] * r]
            }
            Filter | AutoFilter | LowPass | HighPass | BandPass | VowelFilter => {
                if self.sample_counter % 16 == 0 {
                    let amount = match kind {
                        Filter => 0.5 + 0.5 * (self.phase * TAU).sin() * depth,
                        AutoFilter => (self.envelope * 4.0 * depth).clamp(0.0, 1.0),
                        _ => depth,
                    };
                    let freq = 30.0 * 600.0f32.powf(amount);
                    let q = 0.707 + self.params.size * 2.0;
                    for f in &mut self.filter {
                        f.set(
                            self.sample_rate,
                            if kind == VowelFilter {
                                300.0 + 600.0 * amount
                            } else {
                                freq
                            },
                            q,
                        );
                    }
                    for f in &mut self.filter2 {
                        f.set(self.sample_rate, 2300.0 - 1400.0 * amount, 4.0);
                    }
                }
                std::array::from_fn(|ch| {
                    if kind == VowelFilter {
                        return (band(&mut self.filter[ch], input[ch])
                            + band(&mut self.filter2[ch], input[ch]))
                            * 0.8;
                    }
                    let (lo, hi) = self.filter[ch].process(input[ch]);
                    match kind {
                        HighPass => hi,
                        BandPass => input[ch] - lo - hi,
                        _ => lo,
                    }
                })
            }
            Crush => {
                if self.sample_counter % self.crush_hold == 0 {
                    self.held = input.map(|s| (s * self.crush_levels).round() / self.crush_levels);
                }
                self.held
            }
            Dist | Fuzz | Overdrive => std::array::from_fn(|ch| {
                let gain = 1.0 + drive * if kind == Fuzz { 40.0 } else { 19.0 };
                let x = if kind == Fuzz {
                    (input[ch] * gain).clamp(-0.8, 0.8)
                } else if kind == Overdrive {
                    (input[ch] * gain + 0.2).tanh() - 0.2f32.tanh()
                } else {
                    (input[ch] * gain).tanh()
                };
                let y = x - self.dc_input[ch] + 0.995 * self.dc_output[ch];
                self.dc_input[ch] = x;
                self.dc_output[ch] = flush_small(y);
                y
            }),
            Pitch | Granulizer => self.pitch.process(input, kind == Granulizer),
            Spectralizer => self.spectral.process(input, depth),
            Vocoder => self.vocoder.process(input),
            RingMod => {
                let c = (self.carrier_phase * TAU).sin();
                input.map(|x| x * c)
            }
            Compressor => {
                let threshold = 10.0f32.powf((-6.0 - depth * 30.0) / 20.0);
                let desired = if self.envelope > threshold {
                    (threshold / self.envelope).powf(0.75)
                } else {
                    1.0
                };
                let k = if desired < self.dynamics_gain {
                    self.attack
                } else {
                    self.release
                };
                self.dynamics_gain += k * (desired - self.dynamics_gain);
                input.map(|x| x * self.dynamics_gain)
            }
            NoiseGate => {
                let threshold = 10.0f32.powf((-60.0 + depth * 50.0) / 20.0);
                if self.envelope > threshold {
                    self.gate_open = true;
                } else if self.envelope < threshold * 0.7 {
                    self.gate_open = false;
                }
                let desired = if self.gate_open { 1.0 } else { 0.0 };
                let k = if self.gate_open {
                    self.attack
                } else {
                    self.release
                };
                self.dynamics_gain += k * (desired - self.dynamics_gain);
                input.map(|x| x * self.dynamics_gain)
            }
            Noise | NoiseSweep | GatedNoise | NoisePump | NoiseFollower | Riser => {
                let amount = if kind == Riser { t.min(1.0) } else { depth };
                if self.sample_counter % 16 == 0 {
                    for f in &mut self.filter {
                        f.set(self.sample_rate, 80.0 * 180.0f32.powf(amount), 0.8);
                    }
                }
                let gain = match kind {
                    GatedNoise => {
                        self.gate[0].set(if self.phase < 0.1 + depth * 0.8 {
                            1.0
                        } else {
                            0.0
                        });
                        self.gate[0].next()
                    }
                    NoisePump => 1.0 - (-self.phase * 7.0).exp(),
                    NoiseFollower => self.envelope.min(1.0),
                    Riser => {
                        if t < 1.0 {
                            t * (1.0 - t).min(0.01) * 100.0
                        } else {
                            0.0
                        }
                    }
                    _ => 1.0,
                };
                std::array::from_fn(|ch| {
                    let n = self.noise_sample();
                    let n = if kind == Noise {
                        n
                    } else {
                        self.filter[ch].process(n).0
                    };
                    n * drive * gain * 0.5
                })
            }
            Fader => input.map(|x| x * (1.0 - t).max(0.0)),
            Macro => {
                if self.sample_counter % 16 == 0 {
                    for f in &mut self.filter {
                        f.set(self.sample_rate, 18000.0 * 0.003f32.powf(depth), 0.8);
                    }
                }
                let tap = self.delay_tap();
                self.delay
                    .push(std::array::from_fn(|ch| input[ch] * feed + tap[ch] * fb));
                let room = self.reverb.process(input.map(|x| x * feed));
                std::array::from_fn(|ch| {
                    let n = self.noise_sample() * drive * depth * 0.04;
                    self.filter[ch].process(input[ch]).0 * (1.0 - depth)
                        + tap[ch] * depth * 0.4
                        + room[ch] * depth * 0.4
                        + n
                })
            }
        };
        self.phase = (self.phase + self.phase_step).fract();
        std::array::from_fn(|ch| {
            if send {
                wet[ch] * mix
            } else {
                input[ch] + mix * (wet[ch] - input[ch])
            }
        })
    }
}
