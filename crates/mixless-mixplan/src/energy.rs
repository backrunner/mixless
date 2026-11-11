//! Foreground continuity at the moment the outgoing source disappears.
//! Compare local energy to each recording's own typical active level so
//! mastering gain cannot disguise an empty intro or bias track selection.
use mixless_protocol::TrackAnalysis;

mod arrangement;
pub(crate) use arrangement::loss_cost;

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

/// Exact-window intensity difference, independent of mastering gain. Give a
/// rise more room than an unearned loss of broadband/rhythmic body.
pub(crate) fn contrast_cost(a: &TrackAnalysis, b: &TrackAnalysis, out: f32, input: f32) -> f32 {
    let span_a = 2. * crate::grid::Grid(a).meter() * 60. / a.tempo.global_bpm.max(20.);
    let span_b = 2. * crate::grid::Grid(b).meter() * 60. / b.tempo.global_bpm.max(20.);
    match (
        mixless_protocol::musical_energy(a, (out - span_a).max(0.), out),
        mixless_protocol::musical_energy(b, input, (input + span_b).min(b.duration_sec)),
    ) {
        (Some(a), Some(b)) => {
            let loss = a.drive_db - b.drive_db;
            (loss.abs() / 12.).min(1.) * if loss > 0. { 0.18 } else { 0.08 }
        }
        _ => 0.,
    }
}

/// A destination labelled Drop does not supply a build in the outgoing song.
/// Allow the rise its own next phrase would have produced; price only the
/// excess. All windows use local source clocks, including tempo changes.
pub(crate) fn unearned_rise(a: &TrackAnalysis, b: &TrackAnalysis, out: f32, input: f32) -> f32 {
    let (ga, gb) = (crate::grid::Grid(a), crate::grid::Grid(b));
    let before =
        mixless_protocol::musical_energy(a, ga.sec((ga.beat(out) - 2. * ga.meter()).max(0.)), out);
    let after = mixless_protocol::musical_energy(
        b,
        input,
        gb.sec(gb.beat(input) + 2. * gb.meter()).min(b.duration_sec),
    );
    let natural = mixless_protocol::musical_energy(
        a,
        out,
        ga.sec(ga.beat(out) + 2. * ga.meter()).min(a.duration_sec),
    );
    match (before, after) {
        (Some(before), Some(after)) => {
            let expected = natural.map_or(before.drive_db + 3., |e| {
                (e.drive_db + 1.5).max(before.drive_db + 3.)
            });
            ((after.drive_db - expected) / 6.).clamp(0., 1.)
        }
        _ => 0.,
    }
}

#[cfg(test)]
#[test]
fn a_drop_label_does_not_earn_a_rise_but_an_actual_outgoing_build_does() {
    use mixless_protocol::SectionLabel as L;
    for tempo in [90., 120., 175.] {
        for gain_a in [0.25f32, 1., 2.] {
            for gain_b in [0.25f32, 1., 2.] {
                let mut a = crate::tests::track(1, tempo, "8A", L::BuildUp, 64, 0.9, 0.3 * gain_a);
                let b = crate::tests::track(2, tempo, "8A", L::Drop, 64, 0.9, 0.3 * gain_b);
                for bar in &mut a.bars[14..16] {
                    bar.rms = 0.1 * gain_a;
                }
                for bar in &mut a.bars[16..18] {
                    bar.rms = 0.15 * gain_a;
                }
                let out = a.bars[16].start_sec;
                assert!(unearned_rise(&a, &b, out, 0.) > 0.5);
                for bar in &mut a.bars[16..18] {
                    bar.rms = 0.3 * gain_a;
                }
                assert_eq!(
                    unearned_rise(&a, &b, out, 0.),
                    0.,
                    "keep a real build resolution"
                );
                for bar in &mut a.bars[14..16] {
                    bar.rms = 0.3 * gain_a;
                }
                assert_eq!(unearned_rise(&a, &b, out, 0.), 0., "matched foregrounds");
                a.bars.clear();
                assert_eq!(
                    unearned_rise(&a, &b, out, 0.),
                    0.,
                    "missing evidence cannot assert a surge"
                );
            }
        }
    }
}

#[cfg(test)]
#[test]
fn spectral_drive_distinguishes_rumble_and_survives_gain_and_grid_aliases() {
    use mixless_protocol::{
        bar_energy, energy_reference, musical_energy, track_energy, SectionLabel as L,
    };
    let mut full = crate::tests::track(1, 150., "8A", L::Drop, 32, 0.9, 0.3);
    for b in &mut full.bars {
        b.low_db = -12.;
        b.mid_db = -12.;
        b.high_db = -14.;
        b.onset_density = 40.;
    }
    let mut bass = full.clone();
    for b in &mut bass.bars {
        b.low_db += 6.;
        b.mid_db -= 6.;
        b.high_db -= 6.;
        b.onset_density *= 0.5;
    }
    assert!(track_energy(&full).unwrap() > track_energy(&bass).unwrap() + 3.);
    let reference = track_energy(&full).unwrap();
    for gain in [0.25f32, 1., 2.] {
        let mut changed = full.clone();
        changed.tempo.global_bpm *= 0.5;
        for b in &mut changed.bars {
            b.rms *= gain;
            b.low_db += 20. * gain.log10();
            b.mid_db += 20. * gain.log10();
            b.high_db += 20. * gain.log10();
            b.start_sec += 37.;
            b.end_sec += 37.;
        }
        assert!((track_energy(&changed).unwrap() - reference).abs() < 0.0001);
        assert!(
            (musical_energy(&changed, 37., changed.duration_sec + 37.)
                .unwrap()
                .drive_db
                - reference)
                .abs()
                < 0.0001
        );
    }
    let e = bar_energy(&full.bars[0], energy_reference(&full));
    assert!(e.drive_db.is_finite() && (0. ..=1.).contains(&e.activity));
    assert!(musical_energy(&full, -4., -1.).is_none());
}

/// A hard cut should resolve into music, not
/// restart the last build phrase. Layered blends may introduce that same build
/// early; explicit user cues remain authoritative at the caller.
pub(crate) fn cut_entry_supported(b: &TrackAnalysis, input: f32) -> bool {
    let gb = crate::grid::Grid(b);
    let bar = gb.meter() * 60. / gb.bpm(gb.beat(input));
    !b.sections.iter().any(|s| {
        s.label == mixless_protocol::SectionLabel::BuildUp
            && input < s.end_sec - 0.05
            && input >= s.start_sec - bar - 0.05
            && s.end_sec - input <= 16. * bar
            && crate::drops::next_peak_start(b, s.end_sec)
                .is_some_and(|at| (at - s.end_sec).abs() < 0.08)
    })
}

#[cfg(test)]
#[test]
fn cuts_wait_for_the_build_release_but_keep_intro_and_drop_entries() {
    use mixless_protocol::{Section, SectionLabel as L};
    let mut t = crate::tests::track(1, 120., "8A", L::Intro, 64, 0.9, 0.3);
    t.sections = [
        (0., 32., L::Intro),
        (32., 48., L::BuildUp),
        (48., 96., L::Drop),
        (96., 128., L::Outro),
    ]
    .into_iter()
    .map(|(start_sec, end_sec, label)| Section {
        start_sec,
        end_sec,
        label,
    })
    .collect();
    assert!(cut_entry_supported(&t, 0.));
    assert!(!cut_entry_supported(&t, 30.));
    assert!(!cut_entry_supported(&t, 32.));
    assert!(!cut_entry_supported(&t, 44.));
    assert!(cut_entry_supported(&t, 48.));
}
