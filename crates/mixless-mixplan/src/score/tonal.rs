use super::{compatibility, shifted_camelot};
use mixless_protocol::{tonal_intervals, tonal_profile, TrackAnalysis};

/// How much tonal overlap a pair's measured evidence permits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tonality {
    /// Confident compatible keys: tonal parts may layer freely.
    Compatible,
    /// Weak labels without measured conflict: a quiet tail may overlap, and
    /// both foregrounds share at most one bar above -12 dB.
    Unknown,
    /// Confident or measured conflict: one bar of complementary exchange.
    Conflict,
}

/// Semitone and tritone collisions between two pitch-class distributions,
/// independent of level. Compatible material measures about 0.14 and
/// conflicting keys about 0.22 on real chroma; scales still share notes.
pub(crate) fn dissonance(x: &[f32; 12], y: &[f32; 12]) -> Option<f32> {
    let (sx, sy) = (x.iter().sum::<f32>(), y.iter().sum::<f32>());
    if !(sx > 1e-8 && sy > 1e-8) {
        return None;
    }
    let mut total = 0.;
    for i in 0..12 {
        for j in 0..12 {
            total += x[i]
                * y[j]
                * match (i + 12 - j) % 12 {
                    1 | 11 => 1.,
                    6 => 0.5,
                    _ => 0.,
                };
        }
    }
    Some(total / (sx * sy))
}

/// Above this, simultaneous local chroma is treated as a measured conflict.
pub(crate) const DISSONANT: f32 = 0.15;

/// A transposition for B, within the automatic limit, that measured simultaneous
/// chroma supports when key labels are too weak to choose one. It must reach
/// the compatible range and clearly improve on the native pitch. Pitch
/// recovery may later restore B's original key in a solo window.
pub(crate) fn measured_shift(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    ra: (f32, f32),
    rb: (f32, f32),
    pa: f32,
    pb: f32,
) -> Option<f32> {
    let native = overlap_dissonance(a, b, ra, rb, pa, pb)?;
    let detune = pa - pa.round();
    (-6..=6)
        .filter_map(|n| {
            let target = n as f32 + detune;
            let shift = target - pb;
            (shift.abs() > 0.5 && target.abs() <= super::MAX_KEY_SHIFT + 0.5)
                .then(|| overlap_dissonance(a, b, ra, rb, pa, target).map(|d| (shift, d)))
                .flatten()
        })
        .filter(|(_, d)| *d <= DISSONANT - 0.02 && native - *d >= 0.03)
        .min_by(|x, y| (x.1 + 0.005 * x.0.abs()).total_cmp(&(y.1 + 0.005 * y.0.abs())))
        .map(|(shift, _)| shift)
}

/// Confident incompatible labels stay a conflict whatever the chroma says.
/// Weak labels need low measured collision before any tonal layering, and a
/// measured transposition reaches only that bounded tier.
pub(crate) fn tonality(harmonic: bool, reliable: bool, dissonance: Option<f32>) -> Tonality {
    if harmonic {
        Tonality::Compatible
    } else if !reliable && dissonance.is_some_and(|d| d <= DISSONANT) {
        Tonality::Unknown
    } else {
        Tonality::Conflict
    }
}

/// Time-aligned local chroma collision across the two overlap ranges, after
/// the sounding pitch offsets. Each instant compares what plays together;
/// one averaged profile would hide a conflicting half under a matching one.
pub(crate) fn overlap_dissonance(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    ra: (f32, f32),
    rb: (f32, f32),
    pa: f32,
    pb: f32,
) -> Option<f32> {
    if a.tonal_frames.is_empty() || b.tonal_frames.is_empty() || ra.1 <= ra.0 || rb.1 <= rb.0 {
        return None;
    }
    let shift = ((pb - pa).round() as i32).rem_euclid(12) as usize;
    let at = |t: &TrackAnalysis, sec: f32| {
        let i = t.tonal_frames.partition_point(|f| f.end_sec <= sec);
        t.tonal_frames
            .get(i)
            .filter(|f| f.start_sec <= sec)
            .map(|f| f.chroma)
    };
    let mut values = vec![];
    for k in 0..32 {
        let x = (k as f32 + 0.5) / 32.;
        let (Some(ca), Some(cb)) = (
            at(a, ra.0 + (ra.1 - ra.0) * x),
            at(b, rb.0 + (rb.1 - rb.0) * x),
        ) else {
            continue;
        };
        let cb: [f32; 12] = std::array::from_fn(|i| cb[(i + 12 - shift) % 12]);
        values.extend(dissonance(&ca, &cb));
    }
    // Half the window must be observed; silence cannot certify a blend.
    (values.len() >= 16).then(|| values.iter().sum::<f32>() / values.len() as f32)
}

/// Compare simultaneous local contexts, splitting at boundaries on either
/// source clock. A single pitch correction must fit every part of the overlap;
/// averaging a modulation into a third key cannot authorize that correction.
pub(crate) fn overlap_key_match(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    ra: (f32, f32),
    rb: (f32, f32),
    pa: f32,
    pb: f32,
) -> Option<(f32, f32, bool)> {
    if ![ra.0, ra.1, rb.0, rb.1, pa, pb]
        .iter()
        .all(|v| v.is_finite())
        || ra.1 <= ra.0
        || rb.1 <= rb.0
    {
        return Some((0., 0., false));
    }
    // Legacy analyses without any local pitch observations can still use the
    // existing region fallback. A measured but unpitched window cannot.
    let has_local = |t: &TrackAnalysis| {
        !t.tonal_frames.is_empty() || t.bars.iter().any(|bar| bar.chroma.iter().any(|v| *v > 0.))
    };
    if !has_local(a) && !has_local(b) {
        return None;
    }
    if !has_local(a) || !has_local(b) {
        return Some((0., 0., false));
    }
    let mut edges = vec![0., 1.];
    for (t, range) in [(a, ra), (b, rb)] {
        edges.extend(
            tonal_intervals(t, range.0, range.1)
                .into_iter()
                .flat_map(|(start, end)| [start, end])
                .map(|sec| ((sec - range.0) / (range.1 - range.0)).clamp(0., 1.)),
        );
    }
    edges.sort_by(f32::total_cmp);
    edges.dedup_by(|a, b| (*a - *b).abs() < 0.0001);
    let mut pieces = Vec::new();
    let mut reliable = true;
    for edge in edges.windows(2) {
        let profile = |t: &TrackAnalysis, r: (f32, f32)| {
            tonal_profile(t, r.0 + (r.1 - r.0) * edge[0], r.0 + (r.1 - r.0) * edge[1])
        };
        let x = profile(a, ra);
        let y = profile(b, rb);
        let certainty = x.harmonic_confidence.min(y.harmonic_confidence);
        reliable &= certainty >= 0.55;
        pieces.push((
            x.candidates.first().map(|c| c.camelot.clone()),
            y.candidates.first().map(|c| c.camelot.clone()),
            certainty,
            edge[1] - edge[0],
        ));
    }
    let fit = |pitch: f32| {
        let mut score = 0.;
        let mut all = true;
        for (ka, kb, certainty, weight) in &pieces {
            let c = ka
                .as_deref()
                .zip(kb.as_deref())
                .and_then(|(ka, kb)| {
                    shifted_camelot(ka, pa.round()).zip(shifted_camelot(kb, pitch.round()))
                })
                .map_or(0., |(a, b)| compatibility(a, b));
            all &= c >= 0.85;
            score += weight * (certainty * c + (1. - certainty) * 0.5);
        }
        (score, all)
    };
    let (native, compatible) = fit(pb);
    if !reliable {
        return Some((native, 0., false));
    }
    let detune = pa - pa.round();
    if compatible && (detune - (pb - pb.round())).abs() <= 0.005 {
        // Preserve the established score scale for confident matches.
        let exact = pieces.iter().all(|(x, y, _, _)| {
            x.as_deref().zip(y.as_deref()).is_some_and(|(x, y)| {
                shifted_camelot(x, pa.round()) == shifted_camelot(y, pb.round())
            })
        });
        return Some((if exact { 1. } else { 0.85 }, 0., true));
    }
    let shift = (-6..=6)
        .filter_map(|n| {
            let target = n as f32 + detune;
            (target.abs() <= super::MAX_KEY_SHIFT + 0.5 && fit(target).1).then_some(target - pb)
        })
        .min_by(|a, b| a.abs().total_cmp(&b.abs()));
    Some(match shift {
        Some(shift) => (0.5 / shift.abs().max(1.), shift, true),
        None => (0., 0., true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{tonal_windows, Section, SectionLabel as S, TonalFrame};

    fn chroma(root: usize) -> [f32; 12] {
        let c = [
            6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
        ];
        std::array::from_fn(|i| c[(i + 12 - root) % 12])
    }
    fn track(keys: &[usize]) -> TrackAnalysis {
        let mut t = crate::tests::track(1, 120., "8B", S::Intro, 32, 0.8, 0.2);
        t.sections.clear();
        t.tonal_frames = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                t.sections.push(Section {
                    start_sec: i as f32 * 8.,
                    end_sec: (i + 1) as f32 * 8.,
                    label: S::Verse,
                });
                TonalFrame {
                    start_sec: i as f32 * 8.,
                    end_sec: (i + 1) as f32 * 8.,
                    chroma: chroma(*k),
                }
            })
            .collect();
        t
    }
    #[test]
    fn local_overlap_overrides_identical_global_tags_and_cannot_average_away_modulation() {
        let a = track(&[0, 0]);
        let b = track(&[0, 6]);
        assert_eq!(
            overlap_key_match(&a, &b, (0., 8.), (0., 8.), 0., 0.),
            Some((1., 0., true))
        );
        let tail = overlap_key_match(&a, &b, (8., 16.), (8., 16.), 0., 0.).unwrap();
        assert!(tail.1 != 0. && tail.2);
        assert_eq!(
            overlap_key_match(&a, &b, (0., 16.), (0., 16.), 0., 0.),
            Some((0., 0., true))
        );
    }
    #[test]
    fn unobserved_and_flat_parts_never_borrow_the_global_key_or_prepare_pitch() {
        let a = track(&[0, 0]);
        let mut b = track(&[2, 2]);
        b.tonal_frames[1].chroma = [1.; 12];
        let p = overlap_key_match(&a, &b, (0., 16.), (0., 16.), 0., 0.).unwrap();
        assert_eq!(p.1, 0.);
        assert!(!p.2);
        b.tonal_frames.pop();
        let p = overlap_key_match(&a, &b, (8., 16.), (8., 16.), 0., 0.).unwrap();
        assert_eq!(p.1, 0.);
        assert!(!p.2);
    }

    #[test]
    fn a_short_conflicting_tail_cannot_hide_in_a_confident_average() {
        let a = track(&[0, 0]);
        let mut b = track(&[0, 6]);
        b.tonal_frames[0].end_sec = 14.4;
        b.tonal_frames[1].start_sec = 14.4;
        b.sections[0].end_sec = 14.4;
        b.sections[1].start_sec = 14.4;
        let average = tonal_profile(&b, 0., 16.);
        assert_eq!(average.candidates[0].camelot, "8B");
        assert!(average.harmonic_confidence > 0.55);
        assert_eq!(
            overlap_key_match(&a, &b, (0., 16.), (0., 16.), 0., 0.),
            Some((0., 0., true))
        );
    }
    #[test]
    fn matched_modulations_survive_time_gain_and_grid_transformations() {
        for speed in [0.8, 1., 1.2] {
            for gain in [0.01, 1., 100.] {
                for shift in [0., 31.] {
                    let mut a = track(&[0, 6]);
                    let mut b = track(&[0, 6]);
                    for t in [&mut a, &mut b] {
                        for f in &mut t.tonal_frames {
                            f.start_sec = f.start_sec / speed + shift;
                            f.end_sec = f.end_sec / speed + shift;
                            f.chroma = f.chroma.map(|v| v * gain);
                        }
                        for s in &mut t.sections {
                            s.start_sec = s.start_sec / speed + shift;
                            s.end_sec = s.end_sec / speed + shift;
                        }
                    }
                    b.tempo.global_bpm *= 0.5;
                    assert_eq!(
                        overlap_key_match(
                            &a,
                            &b,
                            (shift, shift + 16. / speed),
                            (shift, shift + 16. / speed),
                            0.35,
                            0.35
                        ),
                        Some((1., 0., true))
                    );
                }
            }
        }
    }
    #[test]
    fn sections_keep_distinct_keys_and_candidates_without_inventing_an_average_key() {
        let t = track(&[0, 6]);
        let windows = tonal_windows(&t, 0., 16.);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].profile.candidates[0].camelot, "8B");
        assert_eq!(windows[1].profile.candidates[0].camelot, "2B");
        let (_, cam, confidence) = mixless_protocol::local_tonality(&t, 0., 16.);
        assert!(["8B", "2B"].contains(&cam.as_deref().unwrap()));
        assert!(confidence <= 0.5);
        let exact = tonal_profile(&t, 8.1, 15.9);
        assert_eq!(exact.candidates[0].camelot, "2B");
        assert_eq!(exact.candidates.len(), 3);
    }
}
