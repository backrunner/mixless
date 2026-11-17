//! Streaming measurements of the actual playback graph, outside the callback.
use super::*;
use ebur128::{EbuR128, Mode};

#[derive(Clone, Copy, Default, Debug)]
pub(super) struct Metric {
    // K-weighted stereo power, then phase-aligned low/mid/high powers.
    pub power: [f64; 4],
    pub short: f64,
    pub peak: f64,
    pub reduction: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_measurements_are_independent_of_readout_cadence() {
        for sr in [16_000, 44_100, 48_000] {
            let hop = (HOP * sr as f32) as usize / STEP * STEP;
            let mut coarse = Meter::new(sr, hop).unwrap();
            let mut fine = Meter::new(sr, hop).unwrap();
            let mut pending = 0;
            let mut phase = 1;
            let mut peak = 0f64;
            let mut fine_bands = Vec::new();
            for start in (0..hop * 16).step_by(STEP) {
                let mut routed = [[0.; 8]; STEP];
                let mut output = [0.; STEP * 2];
                for i in 0..STEP {
                    let t = (start + i) as f32 / sr as f32;
                    let a = (std::f32::consts::TAU * 60. * t).sin()
                        * 0.1
                        * if (t * 13.).fract() < 0.17 { 1. } else { 0.1 };
                    let b = (std::f32::consts::TAU * 3000. * t).sin() * 0.03;
                    routed[i] = [a, a, b, b, a + b, a + b, 1., 1.];
                    output[i * 2..i * 2 + 2].fill(a + b);
                }
                coarse.push(&routed, &output, 0.);
                fine.push(&routed, &output, 0.);
                pending += STEP;
                if pending >= (hop / STEP * phase).div_ceil(4) * STEP {
                    let measured = fine.finish().unwrap();
                    fine_bands.append(&mut fine.output_bands);
                    peak = peak.max(measured[4].peak);
                    if pending >= hop {
                        let expected = coarse.finish().unwrap();
                        assert_eq!(coarse.output_bands.len(), hop / STEP);
                        assert_eq!(coarse.output_bands, fine_bands);
                        coarse.output_bands.clear();
                        fine_bands.clear();
                        for d in 0..5 {
                            for b in 0..4 {
                                assert!(
                                    (expected[d].power[b] - measured[d].power[b]).abs() < 1e-10
                                );
                            }
                            assert!((expected[d].short - measured[d].short).abs() < 1e-10);
                        }
                        assert!((expected[4].peak - peak).abs() < 1e-7);
                        peak = 0.;
                        pending = 0;
                        phase = 1;
                    } else {
                        phase += 1;
                    }
                }
            }
        }
    }
}
pub(super) type Reading = [Metric; 5]; // routed A, B, A+B, pre-master, output

pub(super) struct Meter {
    loudness: [EbuR128; 5],
    bands: [[Isolator; 2]; 4],
    band_history: std::collections::VecDeque<[[f64; 3]; 5]>,
    band_sum: [[f64; 3]; 5],
    window_steps: usize,
    pub(super) output_bands: Vec<[f64; 3]>,
    samples: [Vec<f32>; 5],
    reduction: [f64; 5],
}
impl Meter {
    pub fn new(sr: u32, hop: usize) -> Option<Self> {
        let meters: Vec<_> = (0..5)
            .map(|i| {
                EbuR128::new(
                    2,
                    sr,
                    Mode::M
                        | Mode::S
                        | if i == 4 {
                            Mode::TRUE_PEAK
                        } else {
                            Mode::empty()
                        },
                )
                .ok()
            })
            .collect::<Option<_>>()?;
        Some(Self {
            loudness: meters.try_into().ok()?,
            bands: std::array::from_fn(|_| std::array::from_fn(|_| Isolator::new(sr as f32))),
            band_history: std::collections::VecDeque::with_capacity(hop * 2 / STEP + 1),
            band_sum: [[0.; 3]; 5],
            window_steps: (hop * 2 / STEP).max(1),
            output_bands: Vec::with_capacity(hop / STEP + 1),
            samples: std::array::from_fn(|_| Vec::with_capacity(hop * 2)),
            reduction: [0.; 5],
        })
    }
    pub fn push(&mut self, routed: &[[f32; 8]], output: &[f32], master_reduction: f32) {
        self.reduction[4] = self.reduction[4].max(master_reduction as f64);
        for d in 0..2 {
            let gain = routed.iter().map(|s| s[6 + d]).fold(1f32, f32::min);
            self.reduction[d] = self.reduction[d].max(-20. * f64::from(gain.max(1e-12)).log10());
        }
        for (i, s) in routed.iter().enumerate() {
            for (dst, pair) in self.samples.iter_mut().zip([
                [s[0], s[1]],
                [s[2], s[3]],
                [s[0] + s[2], s[1] + s[3]],
                [s[4], s[5]],
                [output[i * 2], output[i * 2 + 1]],
            ]) {
                dst.extend_from_slice(&pair);
            }
        }
    }
    pub fn finish(&mut self) -> Option<Reading> {
        let frames = self.samples[0].len() / 2;
        if frames == 0 || frames % STEP != 0 {
            return None;
        }
        let mut powers = [[0.; 3]; 5];
        for i in 0..frames {
            for ch in 0..2 {
                let split: [[f32; 3]; 4] = std::array::from_fn(|d| {
                    let stream = [0, 1, 3, 4][d];
                    self.bands[d][ch].split(self.samples[stream][i * 2 + ch])
                });
                for b in 0..3 {
                    for (d, x) in [
                        split[0][b],
                        split[1][b],
                        split[0][b] + split[1][b],
                        split[2][b],
                        split[3][b],
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        powers[d][b] += f64::from(x).powi(2);
                    }
                }
            }
            if (i + 1) % STEP == 0 {
                // Keep an exact 400 ms rolling power window in audio blocks.
                // Reading every 50 ms must not silently shorten the window or
                // average unequal warmup chunks as if they had equal duration.
                for d in 0..5 {
                    for b in 0..3 {
                        self.band_sum[d][b] += powers[d][b];
                    }
                }
                self.band_history.push_back(powers);
                if self.band_history.len() > self.window_steps {
                    let old = self.band_history.pop_front()?;
                    for d in 0..5 {
                        for b in 0..3 {
                            self.band_sum[d][b] -= old[d][b];
                        }
                    }
                }
                powers = [[0.; 3]; 5];
                self.output_bands.push(
                    self.band_sum[4].map(|p| p.max(0.) / (self.window_steps * STEP * 2) as f64),
                );
            }
        }
        let mut result = [Metric::default(); 5];
        for d in 0..5 {
            let meter = &mut self.loudness[d];
            meter.add_frames_f32(&self.samples[d]).ok()?;
            result[d].power[0] = 10f64.powf(meter.loudness_momentary().ok()? / 10.);
            result[d].short = 10f64.powf(meter.loudness_shortterm().ok()? / 10.);
            for b in 0..3 {
                result[d].power[b + 1] =
                    self.band_sum[d][b].max(0.) / (self.window_steps * STEP * 2) as f64;
            }
            if d == 4 {
                result[d].peak = meter
                    .prev_true_peak(0)
                    .ok()?
                    .max(meter.prev_true_peak(1).ok()?);
            }
            result[d].reduction = self.reduction[d];
            self.samples[d].clear();
        }
        self.reduction = [0.; 5];
        Some(result)
    }
}
