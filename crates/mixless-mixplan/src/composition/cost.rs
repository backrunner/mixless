//! Source-only reductions are shared by every shape; accumulation order is unchanged.
use super::{power, Frame, Shape};
use crate::score::Tonality;
mod envelopes;
use envelopes::gains;

#[derive(Debug)]
pub(super) enum Rejection {
    /// Voice, tonal overlap and foreground gates do not depend on drum fades.
    Shared,
    /// Routed power includes drums, so this rejection is shape-specific.
    Level,
}

struct Data {
    full: [f32; 2],
    sounding: [[bool; 4]; 2],
    melody: [f32; 2],
    vocal: [f32; 2],
    foreground: [f32; 2],
    weight: [f32; 2],
    dt: f32,
}
pub(super) struct Prepared<'a> {
    frames: &'a [Frame],
    data: Vec<Data>,
    tonality: Tonality,
}
impl<'a> Prepared<'a> {
    pub(super) fn new(frames: &'a [Frame], tonality: Tonality) -> Self {
        let data = frames
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let full = std::array::from_fn(|d| f.bands[d].iter().flatten().sum::<f32>());
                let melody: [f32; 2] = std::array::from_fn(|d| f.bands[d][2][1..].iter().sum());
                let vocal: [f32; 2] = std::array::from_fn(|d| f.bands[d][0].iter().sum());
                // Preserve the original flat sum: full[0] + full[1] would round differently.
                let total = f.bands.iter().flatten().flatten().sum::<f32>().max(1e-12);
                Data {
                    full,
                    melody,
                    vocal,
                    sounding: std::array::from_fn(|d| {
                        std::array::from_fn(|s| f.bands[d][s].iter().sum::<f32>() > full[d] * 0.01)
                    }),
                    foreground: std::array::from_fn(|d| melody[d] + vocal[d]),
                    weight: std::array::from_fn(|d| f.bands[d][2].iter().sum::<f32>() / total),
                    dt: if i == 0 {
                        0.001
                    } else {
                        (f.sec - frames[i - 1].sec).max(0.001)
                    },
                }
            })
            .collect();
        Self {
            frames,
            data,
            tonality,
        }
    }
    /// Relative power and masking, independent of mastering gain. The reference
    /// follows the actual foreground exchange, not a line from the first sample
    /// to a quiet incoming intro: that line would fade an ongoing outgoing peak.
    pub(super) fn score(&self, shape: &Shape) -> Result<f32, Rejection> {
        let frames = self.frames;
        let tonality = self.tonality;
        let mut cost = 0.;
        let mut absent = 0.;
        let mut previous = 0.;
        // Tonal time both decks hold above -12 dB (unknown keys) or at all
        // (conflicting keys): one bar is a crossfade, not a sustained layer.
        let mut shared = 0.;
        for (f, data) in frames.iter().zip(&self.data) {
            let g = gains(shape, f.u);
            if tonality != Tonality::Compatible {
                // Only a part with measurable pitched power (-20 dB within its
                // deck) can clash. An open lane over a silent stem is not a layer.
                let sounding = |d: usize, s: usize| data.sounding[d][s];
                let tonal = |d: usize| {
                    [0, 2, 3]
                        .into_iter()
                        .filter(|s| sounding(d, *s))
                        .map(|s| g[d][s])
                        .fold(0., f32::max)
                };
                // Singers never trade lines across keys that are not confirmed.
                if sounding(0, 0) && sounding(1, 0) && g[0][0] * g[1][0] > 0.0001 {
                    return Err(Rejection::Shared);
                }
                let floor = if tonality == Tonality::Unknown {
                    0.25
                } else {
                    0.01
                };
                if tonal(0) > floor && tonal(1) > floor {
                    shared += f.u - previous;
                    if shared > 1. + 1e-4 {
                        return Err(Rejection::Shared);
                    }
                }
            }
            if f.voice[0].min(f.voice[1]) > 0.35 && g[0][0] * g[1][0] > 0.01 {
                return Err(Rejection::Shared);
            }
            let p = power(f, g);
            let full = |d: usize| data.full[d];
            let progress = g[1][2];
            let target = (full(0) * (1. - progress) + full(1) * progress).max(1e-12);
            let heard = p.iter().flatten().sum::<f32>().max(1e-12);
            // Do not leave a whole bar of drums between two removed melodic beds.
            // A carried singer can own that passage; natural instrumental silence
            // and an audible controlled fade are also legitimate arrangements.
            let melody = |d: usize| data.melody[d];
            let available = melody(0).max(melody(1));
            let bed = melody(0) * g[0][2].powi(2) + melody(1) * g[1][2].powi(2);
            let voice: f32 = (0..2).map(|d| data.vocal[d] * g[d][0].powi(2)).sum();
            let foreground = |d: usize| data.foreground[d];
            let expected = foreground(0) * (1. - progress) + foreground(1) * progress;
            if (available > (full(0) + full(1)) * 0.05
                && bed < available * 0.0001
                && voice < available * 0.125)
                || (expected > (full(0) + full(1)) * 0.05 && bed + voice < expected * 0.25)
            {
                absent += f.u - previous;
                if absent >= 1. {
                    return Err(Rejection::Shared);
                }
            } else {
                absent = 0.;
            }
            previous = f.u;
            let delta = 10. * (heard / target).log10();
            if delta < -6. {
                return Err(Rejection::Level);
            }
            // Peak weight is checked over a complete local bar by
            // foreground::preserved after rendering the actual routes. A single
            // complementary stem-crossing sample may dip below half power without
            // losing that bar's weight; rejecting it here prevents a valid swap.
            cost += delta.min(0.).powi(2) + 0.2 * delta.max(0.).powi(2);
            // Bass is a short, complementary exchange; drums and melody may
            // overlap, but measured competing mid/high power has a real cost.
            for b in [1, 2] {
                cost += 4. * p[0][b].min(p[1][b]) / (p[0][b] + p[1][b]).max(1e-12);
            }
        }
        // A loud chord should not appear simply because a short fade minimized
        // simultaneous power. Price the audible foreground's control movement
        // in wall-clock seconds, while natural attacks at a held gain stay free.
        for (pair, data) in frames.windows(2).zip(self.data.iter().skip(1)) {
            let dt = data.dt;
            for d in 0..2 {
                // Only the melodic fade is used here. Other part envelopes were
                // formerly recomputed by `gains` and immediately discarded.
                let gain = |u| {
                    let x = shape[d][2].at(u);
                    if d == 0 {
                        1. - x
                    } else {
                        x
                    }
                };
                let before = gain(pair[0].u);
                let after = gain(pair[1].u);
                let slope = (after - before).abs() / dt;
                cost += 16. * data.weight[d] * slope.powi(2);
            }
        }
        Ok(cost / frames.len() as f32)
    }
}

#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;
