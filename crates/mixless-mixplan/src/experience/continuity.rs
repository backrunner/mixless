//! Continuous source-time observation; a drum transient is not a sustained layer.
use super::signal;
use mixless_protocol::{MixPlan, TrackAnalysis};

pub(crate) struct Continuity {
    pub overlap_bars: f32,
    pub foreground_gap_bars: f32,
    pub longest_foreground_gap_bars: f32,
    pub last_out_sec: f32,
}

impl Continuity {
    pub(crate) fn gap_cost(&self) -> f32 {
        // Use the existing 0.18 foreground-loss budget. No dilution by a long
        // silent preparation; a fraction of a local bar remains a soft cost.
        0.18 * self.foreground_gap_bars / (1. + self.foreground_gap_bars)
    }
}

pub(crate) fn continuity(p: &MixPlan, a: &TrackAnalysis, b: &TrackAnalysis) -> Option<Continuity> {
    if p.lanes.loop_a.is_some() || p.lanes.loop_b.is_some() {
        return None;
    }
    let reference = [signal::reference(a)?, signal::reference(b)?];
    let n = p.summary.as_ref()?.length_bars as f32;
    let mut result = Continuity {
        overlap_bars: 0.,
        foreground_gap_bars: 0.,
        longest_foreground_gap_bars: 0.,
        last_out_sec: p.t_in_a,
    };
    if n == 0. {
        return Some(result);
    }
    if p.clock.nodes.is_empty() {
        return None;
    }
    // At least 32 samples per master bar, and about 50 ms at slower tempi.
    let steps = (n * 32.).max(p.clock.sample(n) / 0.05).ceil() as usize;
    let du = n / steps.max(1) as f32;
    let mut gap_run = 0.;
    for i in 0..steps {
        let u = (i as f32 + 0.5) * du;
        let decks = [signal::sample(p, a, 0, u)?, signal::sample(p, b, 1, u)?];
        let heard: [f32; 2] =
            std::array::from_fn(|d| decks[d].heard.iter().sum::<f32>() / reference[d]);
        if heard.iter().all(|v| *v > 0.02) {
            result.overlap_bars += du;
        }
        if heard[0] > 0.02 {
            result.last_out_sec = p.source_seconds_at(false, u);
        }
        let raw = (decks[0].raw[0] + decks[0].raw[2]) / reference[0];
        let voice: f32 = (0..2).map(|d| decks[d].heard[0] / reference[d]).sum();
        let melody: f32 = (0..2).map(|d| decks[d].heard[2] / reference[d]).sum();
        // An authored break remains available. Only removal of A's continuing
        // foreground counts; an incoming singer can own the passage instead.
        if raw > 0.02 && voice <= 0.02 && melody + voice < raw * 0.125 {
            result.foreground_gap_bars += du;
            gap_run += du;
            result.longest_foreground_gap_bars = result.longest_foreground_gap_bars.max(gap_run);
        } else {
            gap_run = 0.;
        }
    }
    Some(result)
}

/// Inspect the foreground over one complete master bar. This is used only by
/// order repair, after at least a bar of actual missing content was observed;
/// it does not change pair scores or broaden any musical feasibility gate.
pub(crate) fn minimum_bar_foreground_retention(
    p: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
) -> Option<f32> {
    if p.lanes.loop_a.is_some() || p.lanes.loop_b.is_some() {
        return None;
    }
    let n = p.summary.as_ref()?.length_bars as f32;
    if n < 1. || p.clock.nodes.is_empty() {
        return None;
    }
    let reference = [signal::reference(a)?, signal::reference(b)?];
    let steps = (n * 32.).max(p.clock.sample(n) / 0.05).ceil() as usize;
    let du = n / steps.max(1) as f32;
    let width = (1. / du).ceil() as usize;
    let excess = (width as f32 * du - 1.).max(0.);
    let mut window = vec![[0f32; 2]; width];
    let mut sum = [0f32; 2];
    let mut minimum = 1f32;
    for i in 0..steps {
        let u = (i as f32 + 0.5) * du;
        let decks = [signal::sample(p, a, 0, u)?, signal::sample(p, b, 1, u)?];
        let next = [
            (decks[0].raw[0] + decks[0].raw[2]) / reference[0],
            (0..2)
                .map(|d| (decks[d].heard[0] + decks[d].heard[2]) / reference[d])
                .sum(),
        ];
        for part in 0..2 {
            sum[part] += next[part] - window[i % width][part];
        }
        window[i % width] = next;
        if i + 1 < width {
            continue;
        }
        // The adaptive sampling interval need not divide a bar exactly.
        // Trim the oldest sample's excess rather than widening the window.
        let oldest = window[(i + 1) % width];
        let raw = (sum[0] * du - oldest[0] * excess).max(0.);
        let heard = (sum[1] * du - oldest[1] * excess).max(0.);
        if raw > 0.02 {
            minimum = minimum.min(heard / raw);
        }
    }
    Some(minimum)
}
