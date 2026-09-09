//! Source-time musical decisions shared by the smooth planner's envelopes.
use crate::{grid::Grid, phrasing::boundary_quality};
use mixless_protocol::{BarFeature, Polyline, TrackAnalysis};

pub(crate) fn feature(track: &TrackAnalysis, sec: f32) -> Option<&BarFeature> {
    let i = track.bars.partition_point(|b| b.end_sec <= sec);
    track.bars.get(i).filter(|b| b.start_sec <= sec)
}

/// Weight partial bars by their actual duration; quiet bars are still evidence.
pub(crate) fn average_rms(t: &TrackAnalysis, start: f32, end: f32) -> Option<f32> {
    let mut power = 0.;
    let mut duration = 0.;
    for b in &t.bars {
        let overlap = (b.end_sec.min(end) - b.start_sec.max(start)).max(0.);
        if overlap > 0. && b.rms.is_finite() && b.rms >= 0. {
            power += b.rms * b.rms * overlap;
            duration += overlap;
        }
    }
    (duration > 0. && power > 0.).then(|| (power / duration).sqrt())
}

/// B keeps its trim after handoff, so a quiet intro never authorizes gain that
/// clips a later drop. Missing/invalid peak coverage cannot authorize a boost.
pub(crate) fn incoming_trim(
    b: &TrackAnalysis,
    input: f32,
    a_rms: Option<f32>,
    b_rms: Option<f32>,
) -> f32 {
    let mut peak = 0f32;
    let mut covered = input;
    let mut known = true;
    for bar in b.bars.iter().filter(|bar| bar.end_sec > input) {
        if bar.start_sec > covered + 0.01
            || !bar.crest.is_finite()
            || bar.crest < 1.
            || !bar.rms.is_finite()
            || bar.rms < 0.
        {
            known = false;
        }
        peak = peak.max(bar.rms * bar.crest);
        covered = covered.max(bar.end_sec);
    }
    known &= covered >= b.duration_sec - 0.01;
    let headroom = if known && peak > 0. && peak.is_finite() {
        (20. * (0.707 / peak).log10()).clamp(0., 6.)
    } else {
        0.
    };
    match (a_rms, b_rms) {
        (Some(a), Some(b)) => (20. * (a / b).log10()).clamp(-6., headroom),
        _ => 0.,
    }
}

/// Swap on a shared phrase downbeat with an incoming kick, including 2:1
/// mappings. Ranking uses the material at the exchange, not the track's end.
pub(crate) fn bass_handoff(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    source_a: &Polyline,
    source_b: &Polyline,
    n: f32,
) -> Option<f32> {
    let mut best = None;
    for bar in 1..n as usize {
        let u = bar as f32;
        // Leave room to introduce and remove the secondary layer.
        if u < n * 0.25 || u > n * 0.75 {
            continue;
        }
        let ta = source_a.sample(u);
        let tb = source_b.sample(u);
        let ga = Grid(a);
        let gb = Grid(b);
        if (ga.sec(ga.floor_bar(ta)) - ta).abs() > 0.02
            || (gb.sec(gb.floor_bar(tb)) - tb).abs() > 0.02
        {
            continue;
        }
        let pa = boundary_quality(a, ta);
        let pb = boundary_quality(b, tb);
        let (Some(fa), Some(fb)) = (feature(a, ta - 0.01), feature(b, tb + 0.01)) else {
            continue;
        };
        // Don't remove a driving bassline to reveal an empty intro/breakdown.
        if fa.kick_salience > 0.5 && fb.kick_salience < 0.35 {
            continue;
        }
        let energy = 1. - (fa.kick_salience - fb.kick_salience).abs();
        // An eight-bar blend may have no internal phrase. Its midpoint is
        // still a valid shared downbeat, but measured phrases win decisively.
        let score =
            pa.min(pb) * 2. + energy * 0.4 + fb.kick_salience * 0.2 - (u / n - 0.5).abs() * 0.3;
        if best.is_none_or(|(_, previous)| score > previous) {
            best = Some((u, score));
        }
    }
    best.map(|(u, _)| u)
}

/// Conservative permission for percussion layering across incompatible keys:
/// one entire overlap must be drum-only evidence, without a pitched foreground.
pub(crate) fn percussion_only(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    let mut covered = start;
    for bar in t
        .bars
        .iter()
        .filter(|bar| bar.end_sec > start && bar.start_sec < end)
    {
        if bar.start_sec > covered + 0.01
            || bar.rms <= 0.001
            || bar.vocal_presence > 0.2
            || bar.vocal_confidence.is_some_and(|v| v > 0.25)
            || bar.kick_salience < 0.5
            || bar.onset_density < 0.5
            || bar.chord.is_some()
            || bar.local_key.is_some()
        {
            return false;
        }
        // Normalized chroma should be diffuse for untuned percussion. Missing
        // chroma is unknown, not proof that a melody is absent.
        let sum: f32 = bar.chroma.iter().sum();
        let maximum = bar.chroma.iter().copied().fold(0., f32::max);
        if sum <= 0.001 || maximum / sum > 0.14 {
            return false;
        }
        covered = bar.end_sec.min(end);
    }
    covered >= end - 0.01
}
