//! Frozen pre-optimization arithmetic for bit-for-bit scoring regressions.
use super::super::highpass;
use super::super::{gains, Frame, Shape};
use crate::score::Tonality;
fn power(f: &Frame, g: [[f32; 4]; 2]) -> [[f32; 3]; 2] {
    std::array::from_fn(|d| {
        std::array::from_fn(|b| {
            (0..4)
                .map(|s| f.bands[d][s][b] * g[d][s].powi(2))
                .sum::<f32>()
                / (1. + (highpass(g[d], g[1 - d]) / [70f32, 600., 5000.][b]).powi(4))
        })
    })
}

/// Relative power and masking, independent of mastering gain. The reference
/// follows the actual foreground exchange, not a line from the first sample
/// to a quiet incoming intro: that line would fade an ongoing outgoing peak.
pub(super) fn cost(frames: &[Frame], shape: &Shape, tonality: Tonality) -> Option<f32> {
    let mut cost = 0.;
    let mut absent = 0.;
    let mut previous = 0.;
    // Tonal time both decks hold above -12 dB (unknown keys) or at all
    // (conflicting keys): one bar is a crossfade, not a sustained layer.
    let mut shared = 0.;
    for f in frames {
        let g = gains(shape, f.u);
        if tonality != Tonality::Compatible {
            // Only a part with measurable pitched power (-20 dB within its
            // deck) can clash. An open lane over a silent stem is not a layer.
            let sounding = |d: usize, s: usize| {
                f.bands[d][s].iter().sum::<f32>() > f.bands[d].iter().flatten().sum::<f32>() * 0.01
            };
            let tonal = |d: usize| {
                [0, 2, 3]
                    .into_iter()
                    .filter(|s| sounding(d, *s))
                    .map(|s| g[d][s])
                    .fold(0., f32::max)
            };
            // Singers never trade lines across keys that are not confirmed.
            if sounding(0, 0) && sounding(1, 0) && g[0][0] * g[1][0] > 0.0001 {
                return None;
            }
            let floor = if tonality == Tonality::Unknown {
                0.25
            } else {
                0.01
            };
            if tonal(0) > floor && tonal(1) > floor {
                shared += f.u - previous;
                if shared > 1. + 1e-4 {
                    return None;
                }
            }
        }
        if f.voice[0].min(f.voice[1]) > 0.35 && g[0][0] * g[1][0] > 0.01 {
            return None;
        }
        let p = power(f, g);
        let full = |d: usize| f.bands[d].iter().flatten().sum::<f32>();
        let progress = shape[1][2].at(f.u);
        let target = (full(0) * (1. - progress) + full(1) * progress).max(1e-12);
        let heard = p.iter().flatten().sum::<f32>().max(1e-12);
        // Do not leave a whole bar of drums between two removed melodic beds.
        // A carried singer can own that passage; natural instrumental silence
        // and an audible controlled fade are also legitimate arrangements.
        let melody = |d: usize| f.bands[d][2][1..].iter().sum::<f32>();
        let available = melody(0).max(melody(1));
        let bed = melody(0) * g[0][2].powi(2) + melody(1) * g[1][2].powi(2);
        let voice: f32 = (0..2)
            .map(|d| f.bands[d][0].iter().sum::<f32>() * g[d][0].powi(2))
            .sum();
        let foreground = |d: usize| melody(d) + f.bands[d][0].iter().sum::<f32>();
        let expected = foreground(0) * (1. - progress) + foreground(1) * progress;
        if (available > (full(0) + full(1)) * 0.05
            && bed < available * 0.0001
            && voice < available * 0.125)
            || (expected > (full(0) + full(1)) * 0.05 && bed + voice < expected * 0.25)
        {
            absent += f.u - previous;
            if absent >= 1. {
                return None;
            }
        } else {
            absent = 0.;
        }
        previous = f.u;
        let delta = 10. * (heard / target).log10();
        if delta < -6. {
            return None;
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
    for pair in frames.windows(2) {
        let dt = (pair[1].sec - pair[0].sec).max(0.001);
        let (before, after) = (gains(shape, pair[0].u), gains(shape, pair[1].u));
        let total = pair[1]
            .bands
            .iter()
            .flatten()
            .flatten()
            .sum::<f32>()
            .max(1e-12);
        for d in 0..2 {
            let weight = pair[1].bands[d][2].iter().sum::<f32>() / total;
            let slope = (after[d][2] - before[d][2]).abs() / dt;
            cost += 16. * weight * slope.powi(2);
        }
    }
    Some(cost / frames.len() as f32)
}
