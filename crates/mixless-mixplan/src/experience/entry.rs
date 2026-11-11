//! The incoming passage is established by its sounding content, not silent lanes.
use super::signal;
use mixless_protocol::{MixPlan, TrackAnalysis};

fn present(raw: [f32; 4], heard: [f32; 4], voice: bool, reference: f32) -> bool {
    let total: f32 = raw.iter().sum();
    if total <= reference * 0.01 {
        return true;
    }
    let foreground = raw[0] + raw[2];
    // The same 3 dB complete-bar presence budget used for peak weight. A
    // continuing outgoing kit may supply rhythm, but cannot supply B's melody.
    let content = if foreground > reference * 0.02 {
        heard[0] + heard[2] >= foreground * 0.5
    } else {
        heard.iter().sum::<f32>() >= total * 0.5
    };
    content && (!voice || raw[0] <= total * 0.01 || heard[0] >= raw[0] * 0.5)
}

fn covered(p: &MixPlan, b: &TrackAnalysis, lo: f32, hi: f32, reference: f32) -> Option<bool> {
    let n = p.summary.as_ref()?.length_bars as f32;
    let trim = 10f32.powf(p.lanes.gain_b.sample(n) / 10.).max(1e-12);
    let mut raw = [0.; 4];
    let mut heard = [0.; 4];
    let mut voice = false;
    for i in 0..16 {
        let f = signal::sample(p, b, 1, lo + (hi - lo) * (i as f32 + 0.5) / 16.)?;
        for s in 0..4 {
            raw[s] += f.raw[s] / 16.;
            heard[s] += f.heard[s] / (16. * trim);
        }
        voice |= f.voice;
    }
    Some(present(raw, heard, voice, reference))
}

/// Start of the final established incoming passage, in source seconds. Missing
/// source/stem evidence retains the conservative control-only interpretation.
pub fn established_entry(p: &MixPlan, b: &TrackAnalysis) -> f32 {
    let fallback = super::control_entry(p);
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    if n < 1. || p.incoming_source.nodes.is_empty() || p.lanes.loop_b.is_some() {
        return fallback;
    }
    let Some(reference) = signal::reference(b) else {
        return fallback;
    };
    let trim = 10f32.powf(p.lanes.gain_b.sample(n) / 10.).max(1e-12);
    let mut entry = p.t_in_b;
    for i in (0..(n * 16.) as usize).rev() {
        let u = (i as f32 + 0.5) / 16.;
        let Some(f) = signal::sample(p, b, 1, u) else {
            return fallback;
        };
        if !present(f.raw, f.heard.map(|v| v / trim), f.voice, reference) {
            entry = p.source_seconds_at(true, (i + 1) as f32 / 16.);
            break;
        }
    }
    // A complementary fade during a peak's first bar can still deliver its
    // full-weight opening. Credit it only with B's own content and only when
    // the remaining passage is already established; drums alone cannot do it.
    let grid = crate::grid::Grid(b);
    for (start, _) in crate::drops::peaks(b) {
        let end = grid.sec(grid.beat(start) + grid.meter());
        if start >= p.t_in_b - 0.05
            && start < entry
            && end >= entry
            && end <= p.incoming_source.sample(n) + 0.001
            && covered(
                p,
                b,
                signal::bar_at(p, start),
                signal::bar_at(p, end),
                reference,
            ) == Some(true)
        {
            entry = start;
        }
    }
    entry.clamp(p.t_in_b, p.t_end_b)
}

/// Use the existing skipped-peak cost for peaks hidden by the arrangement too.
/// This is a preference, never permission to expose an incomplete vocal line.
pub(crate) fn arrival_debt(p: &MixPlan, b: &TrackAnalysis) -> f32 {
    let entry = established_entry(p, b);
    0.08 / 1.4
        * crate::drops::skipped(b, entry).saturating_sub(crate::drops::skipped(b, p.t_in_b)) as f32
}
