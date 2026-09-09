//! Incremental capture, repeat/reverse playback and transport-style releases.

use super::EffectKind;

// Capture banks are filled incrementally. Swapping an already-filled bank is
// constant time; rearming never reads data left over from a previous capture.
pub(super) struct Capture {
    banks: [Vec<[f32; 2]>; 2],
    bank: usize,
    filled: usize,
    length: usize,
    ready: bool,
    elapsed: usize,
    position: f64,
    last: [f32; 2],
    edge: usize,
    ramp: usize,
}
impl Capture {
    pub(super) fn new(sr: f32) -> Self {
        Self {
            banks: std::array::from_fn(|_| vec![[0.0; 2]; (sr * 12.0) as usize + 4]),
            bank: 0,
            filled: 0,
            length: 1,
            ready: false,
            elapsed: 0,
            position: 0.0,
            last: [0.0; 2],
            edge: (sr * 0.002) as usize,
            ramp: 0,
        }
    }
    pub(super) fn reset(&mut self) {
        self.filled = 0;
        self.ready = false;
        self.elapsed = 0;
        self.position = 0.0;
        self.last = [0.0; 2];
        self.ramp = 0;
    }
    fn at(&self, p: f64) -> [f32; 2] {
        let p = p.rem_euclid(self.length as f64);
        let i = p as usize;
        let f = (p - i as f64) as f32;
        let b = &self.banks[1 - self.bank];
        std::array::from_fn(|ch| b[i][ch] + f * (b[(i + 1) % self.length][ch] - b[i][ch]))
    }
    pub(super) fn process(
        &mut self,
        input: [f32; 2],
        frames: usize,
        kind: EffectKind,
        depth: f32,
        feedback: f32,
    ) -> [f32; 2] {
        use EffectKind::*;
        if !self.ready {
            self.length = frames.clamp(16, self.banks[0].len() - 2);
        }
        if self.filled < self.length {
            self.banks[self.bank][self.filled] = input;
            self.filled += 1;
        }
        if !self.ready {
            if self.filled < self.length {
                self.last = input;
                return input;
            }
            self.bank = 1 - self.bank;
            self.filled = 0;
            self.ready = true;
            self.ramp = self.edge;
            self.position = if kind == Backspin {
                (self.length - 1) as f64
            } else {
                0.0
            };
        }
        let length = self.length as f64;
        let cycle = self.elapsed / self.length;
        let phase = self.elapsed % self.length;
        if phase == 0 && self.elapsed > 0 {
            let refresh = match kind {
                Reverse | ReverseDelay => true,
                BeatLoop => cycle % 2 == 0,
                SlipRoll => cycle % 4 == 0,
                _ => false,
            };
            if refresh && self.filled == self.length {
                self.bank = 1 - self.bank;
                self.filled = 0;
                self.ramp = self.edge;
            }
        }
        let mut position = self.position;
        let mut gain = 1.0;
        if matches!(kind, VinylBrake | TapeStop | Backspin) {
            let t = (self.elapsed as f64 / length).min(1.0);
            let speed = match kind {
                TapeStop => 1.0 - t,
                Backspin => -3.0 * (1.0 - t).powi(2),
                _ => (1.0 - t).powi(2),
            };
            gain = ((1.0 - t) * 20.0).min(1.0) as f32;
            self.position = (self.position + speed).rem_euclid(length);
        } else {
            if kind == OneShot && self.elapsed >= self.length {
                gain = 0.0;
            }
            let divisions = match kind {
                LoopRoll | Helix => 1usize << cycle.min(5),
                Stutter => 1usize << (1.0 + depth * 4.0).round() as usize,
                Beatmasher | Slicer => 8,
                _ => 1,
            };
            let span = (self.length / divisions).max(16);
            let within = self.elapsed % span;
            if within == 0 {
                self.ramp = self.edge.min(span / 4);
            }
            position = within as f64;
            if matches!(kind, Reverse | ReverseRoll | ReverseDelay | Censor) {
                position = length - 1.0 - position;
            }
            if matches!(kind, Beatmasher | Slicer) {
                let step = self.elapsed / span;
                let i = if kind == Slicer {
                    [0, 2, 1, 3, 4, 6, 5, 7][step % 8]
                } else {
                    [0, 0, 3, 1, 4, 4, 7, 2][step % 8]
                };
                position += (i * span) as f64;
            }
            if kind == Stutter && within as f32 / span as f32 > 0.65 {
                gain = 0.0;
            }
            if kind == Helix {
                gain *= feedback.clamp(0.15, 0.88).powi(cycle.min(100) as i32);
            }
        }
        let mut wet = self.at(position).map(|v| v * gain);
        if self.ramp > 0 {
            let weight = 1.0 / self.ramp as f32;
            wet = std::array::from_fn(|ch| self.last[ch] + weight * (wet[ch] - self.last[ch]));
            self.ramp -= 1;
        }
        self.last = wet;
        self.elapsed = self.elapsed.saturating_add(1);
        wet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_roll_repeats_and_reverse_reverses_sample_order() {
        for kind in [EffectKind::Roll, EffectKind::ReverseRoll] {
            let mut capture = Capture::new(48000.0);
            let mut out = Vec::new();
            for i in 0..1536 {
                out.push(
                    capture.process(
                        [if i < 512 { i as f32 / 2048.0 } else { -0.5 }; 2],
                        512,
                        kind,
                        0.5,
                        0.5,
                    )[0],
                );
            }
            for i in 128..400 {
                assert!((out[511 + i] - out[1023 + i]).abs() < 0.00001);
            }
            let slope = out[900] - out[700];
            assert!(
                if kind == EffectKind::Roll {
                    slope > 0.09
                } else {
                    slope < -0.09
                },
                "{kind:?}: {slope}"
            );
            capture.reset();
            for _ in 0..1500 {
                assert_eq!(capture.process([0.0; 2], 512, kind, 0.5, 0.5), [0.0; 2]);
            }
        }
    }
    #[test]
    fn brake_slows_read_head_then_stops_and_rearms() {
        for kind in [
            EffectKind::TapeStop,
            EffectKind::VinylBrake,
            EffectKind::Backspin,
        ] {
            let mut c = Capture::new(48000.0);
            for i in 0..2047 {
                c.process([i as f32 / 8192.0; 2], 2048, kind, 0.5, 0.5);
            }
            let mut early = 0.0;
            let mut late = 0.0;
            for i in 0..2048 {
                let before = c.position;
                c.process([0.0; 2], 2048, kind, 0.5, 0.5);
                let delta = (c.position - before).rem_euclid(2048.0);
                let step = delta.min(2048.0 - delta);
                if i > 10 && i < 100 && step < 4.0 {
                    early += step;
                }
                if i > 1800 && i < 1890 {
                    late += step;
                }
            }
            assert!(early > late * 5.0, "{kind:?}: {early} -> {late}");
            for _ in 0..100 {
                assert_eq!(c.process([0.5; 2], 2048, kind, 0.5, 0.5), [0.0; 2]);
            }
            c.reset();
            assert_eq!(c.process([0.25; 2], 2048, kind, 0.5, 0.5), [0.25; 2]);
        }
    }
}
