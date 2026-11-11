//! Measured participation in the fitted schedule, in outgoing master bars.
use super::{gains, Frame, Shape};

#[derive(Default)]
pub(super) struct Occupancy {
    pub overlap_bars: f32,
    pub selected_run: f32,
    pub carried_voice: bool,
}

pub(super) fn measure(frames: &[Frame], shape: &Shape, n: f32) -> Occupancy {
    let mut points = vec![0., n];
    points.extend(frames.iter().map(|f| f.u));
    points.extend(shape.iter().flatten().flat_map(|f| [f.start, f.end]));
    points.sort_by(f32::total_cmp);
    points.dedup();
    let mut result = Occupancy::default();
    let mut run = 0f32;
    for w in points.windows(2) {
        let u = (w[0] + w[1]) / 2.;
        let f = &frames[((u / n * 128.).round() as usize).min(128)];
        let g = gains(shape, u);
        let power = f
            .bands
            .map(|deck| deck.map(|bands| bands.iter().sum::<f32>()));
        let full = power.map(|parts| parts.iter().sum::<f32>());
        let heard: [f32; 2] =
            std::array::from_fn(|d| (0..4).map(|s| power[d][s] * g[d][s].powi(2)).sum());
        // At least -20 dB of each source, and of the heard mix. Numerical
        // fade tails or a negligible isolated lane cannot earn useful time.
        let audible =
            (0..2).all(|d| heard[d] > full[d] * 0.01 && heard[d] > (heard[0] + heard[1]) * 0.01);
        if audible {
            result.overlap_bars += w[1] - w[0];
        }
        let selected = audible
            && (0..2).all(|d| {
                let present = |s: usize| power[d][s] > full[d] * 0.0001;
                (0..4).any(|s| present(s) && g[d][s] >= 0.5)
                    && (0..4).any(|s| present(s) && g[d][s] <= 0.01)
            });
        if selected {
            run += w[1] - w[0];
            result.selected_run = result.selected_run.max(run);
        } else {
            run = 0.;
        }
        result.carried_voice |= audible
            && f.voice[0] > 0.35
            && power[0][0] > full[0] * 0.0001
            && power[1][2] > full[1] * 0.0001
            && g[0][0] >= 0.5
            && g[1][2] >= 0.5
            && g[0][2] <= 0.01;
    }
    result
}
