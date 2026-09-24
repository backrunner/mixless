//! Foreground continuity at the moment the outgoing source disappears.
//! Compare local energy to each recording's own typical active level so
//! mastering gain cannot disguise an empty intro or bias track selection.
use mixless_protocol::TrackAnalysis;

fn reference(t: &TrackAnalysis) -> Option<f32> {
    let mut levels: Vec<_> = t
        .bars
        .iter()
        .map(|b| b.rms)
        .filter(|r| *r > 0.001)
        .collect();
    levels.sort_by(f32::total_cmp);
    (!levels.is_empty()).then(|| levels[(levels.len() - 1) * 3 / 4])
}

pub(crate) fn handoff_supported(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    out: f32,
    input: f32,
) -> bool {
    let (Some(ra), Some(rb)) = (reference(a), reference(b)) else {
        return true;
    };
    let ga = crate::grid::Grid(a);
    let gb = crate::grid::Grid(b);
    let span_a = 2. * ga.meter() * 60. / ga.bpm(ga.beat(out));
    let span_b = gb.meter() * 60. / gb.bpm(gb.beat(input));
    let Some(before) = crate::musical::average_rms(a, (out - span_a).max(0.), out) else {
        return true;
    };
    let Some(after) = crate::musical::average_rms(b, input, (input + span_b).min(b.duration_sec))
    else {
        return true;
    };
    let before = before / ra;
    let after = after / rb;
    // A source already in its own release may hand over softly. A strong
    // foreground needs the next source established before it disappears.
    // This is a measured 6 dB continuity budget, independent of section names.
    before < 0.55 || after >= before * 0.5
}

/// The existing ranking budget: reward matched levels, but never mistake
/// two equally quiet windows for an established handoff.
pub(crate) fn balance(levels: [Option<f32>; 2]) -> f32 {
    match levels {
        [Some(a), Some(b)] if a > 0. && b > 0. => {
            (1. - (20. * (a / b).log10()).abs() / 18.).clamp(0., 1.)
        }
        _ => 0.5,
    }
}

pub(crate) fn weakness(track: &TrackAnalysis, rms: Option<f32>) -> f32 {
    rms.zip(crate::musical::average_rms(track, 0., track.duration_sec))
        .filter(|(local, reference)| *local > 0. && *reference > 0.001)
        .map_or(0., |(local, reference)| {
            ((20. * (reference / local).log10() - 3.) / 9.).clamp(0., 1.)
        })
}

/// Fallbacks still compare every safe entry with the source that is leaving.
/// Use the same phrase/energy/position terms as normal candidate ranking.
pub(crate) fn entry_score(a: &TrackAnalysis, b: &TrackAnalysis, out: f32, input: f32) -> f32 {
    let ga = crate::grid::Grid(a);
    let gb = crate::grid::Grid(b);
    let before =
        crate::musical::average_rms(a, ga.sec((ga.beat(out) - 2. * ga.meter()).max(0.)), out);
    let after = crate::musical::average_rms(
        b,
        input,
        gb.sec(gb.beat(input) + gb.meter()).min(b.duration_sec),
    );
    0.12 * crate::phrasing::boundary_quality(b, input) + 0.05 * balance([before, after])
        - 0.25 * weakness(a, before).min(weakness(b, after))
        - 0.03 * input / b.duration_sec
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::SectionLabel as L;

    #[test]
    fn weak_fade_in_cannot_replace_a_strong_phrase_but_can_be_layered_until_ready() {
        for gain_a in [0.25, 1., 2.] {
            for gain_b in [0.25, 1., 2.] {
                for tempo in [90., 120., 175.] {
                    let a = crate::tests::track(1, tempo, "8A", L::Unknown, 64, 0.9, 0.2 * gain_a);
                    let mut b =
                        crate::tests::track(2, tempo, "8A", L::Unknown, 64, 0.9, 0.2 * gain_b);
                    for (i, bar) in b.bars.iter_mut().take(8).enumerate() {
                        bar.rms *= 0.08 + i as f32 * 0.09;
                    }
                    assert!(!handoff_supported(&a, &b, a.bars[32].start_sec, 0.));
                    assert!(handoff_supported(
                        &a,
                        &b,
                        a.bars[32].start_sec,
                        b.bars[8].start_sec
                    ));
                    let mut released = a.clone();
                    for bar in &mut released.bars[30..32] {
                        bar.rms *= 0.1;
                    }
                    assert!(handoff_supported(&released, &b, a.bars[32].start_sec, 0.));
                }
            }
        }
    }
}
