//! Sustained instruments need a musical release, independently of the singer.
use mixless_protocol::{Polyline, TrackAnalysis};

mod notes;
mod search;
pub(crate) use search::Search;

fn coordinate(source: &Polyline, sec: f32) -> f32 {
    let i = source.nodes.partition_point(|p| p.1 < sec);
    if i == 0 {
        return source.nodes.first().map_or(0., |p| p.0);
    }
    if i >= source.nodes.len() {
        return source.nodes.last().unwrap().0;
    }
    let (a, b) = (source.nodes[i - 1], source.nodes[i]);
    a.0 + (b.0 - a.0) * (sec - a.1) / (b.1 - a.1).max(1e-6)
}

fn edges(t: &TrackAnalysis) -> Vec<f32> {
    let mut points: Vec<_> = t
        .sections
        .iter()
        .flat_map(|s| [s.start_sec, s.end_sec])
        .chain(
            t.phrase_boundaries
                .iter()
                .filter(|p| p.confidence >= 0.5 && p.novelty >= 0.15)
                .map(|p| p.time_sec),
        )
        .collect();
    points.sort_by(f32::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    points
}

fn quiet(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    let Some(stems) = &t.stems else {
        return false;
    };
    let first = stems.frames.partition_point(|f| f.end_sec <= start);
    let last = stems.frames.partition_point(|f| f.start_sec < end);
    let fs = &stems.frames[first..last.max(first)];
    let covered: f32 = fs
        .iter()
        .map(|f| (f.end_sec.min(end) - f.start_sec.max(start)).max(0.))
        .sum();
    covered >= (end - start).max(0.01) * 0.95
        && !fs.is_empty()
        && fs.iter().all(|f| {
            let melody = 10f32.powf(f.band_db[2][1] / 10.) + 10f32.powf(f.band_db[2][2] / 10.);
            let total: f32 = f
                .band_db
                .iter()
                .flatten()
                .map(|db| 10f32.powf(*db / 10.))
                .sum();
            melody <= total * 0.001
        })
}

/// Verify the audible envelope after crossfader routing and gain compensation,
/// including legacy stem arrangements that did not use this fitter.
pub(crate) fn preserved(p: &mixless_protocol::MixPlan, ctx: &crate::PlanContext<'_>) -> bool {
    let Some(mix) = &p.stem_mix else {
        return true;
    };
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    if n < 4. {
        return true;
    }
    for (d, t, source, stem, level) in [
        (
            0,
            ctx.outgoing,
            &p.outgoing_source,
            &mix.outgoing[2],
            &p.lanes.gain_a,
        ),
        (
            1,
            ctx.incoming,
            &p.incoming_source,
            &mix.incoming[2],
            &p.lanes.gain_b,
        ),
    ] {
        if d == 0
            && (p.lanes.loop_a.is_some()
                || crate::constraints::user_range(ctx.cues_out, t.sample_rate).is_some())
        {
            continue;
        }
        let reference = 10f32.powf(level.sample(if d == 0 { 0. } else { n }) / 20.);
        let gain = |u| {
            let x = (p.lanes.xfader.sample(u) + 1.) * 0.5;
            (if d == 0 { 1. - x } else { x }).clamp(0., 1.).sqrt()
                * stem.sample(u)
                * 10f32.powf(level.sample(u) / 20.)
                / reference.max(1e-6)
        };
        let samples: Vec<_> = (0..=(n * 32.) as usize).map(|i| i as f32 / 32.).collect();
        let low = samples
            .iter()
            .copied()
            .find(|u| {
                if d == 0 {
                    gain(*u) < 0.9
                } else {
                    gain(*u) > 0.01
                }
            })
            .unwrap_or(n);
        let high = samples
            .iter()
            .copied()
            .find(|u| {
                if d == 0 {
                    gain(*u) < 0.01
                } else {
                    gain(*u) > 0.9
                }
            })
            .unwrap_or(n);
        let (start, end) = (source.sample(low), source.sample(high));
        if quiet(t, start, end) {
            continue;
        }
        let grid = crate::grid::Grid(t);
        // A new section's arrangement starts its own notes on that downbeat;
        // revealing it there, at once, cannot expose a note already sounding.
        if d == 1
            && t.sections
                .iter()
                .any(|s| (grid.beat(s.start_sec) - grid.beat(start)).abs() <= grid.meter() * 0.0625)
            && !crate::vocals::continues(t, start)
        {
            continue;
        }
        let bars = (grid.beat(end) - grid.beat(start)) / grid.meter();
        if bars < 0.75 {
            return false;
        }
        // A relative note rest is not silence. Keep the existing minimum
        // audible fade length before considering its more precise timing.
        if notes::supports(t, start, end) {
            continue;
        }
        let anchor = if d == 0 { end } else { start };
        let mut points = edges(t);
        if d == 1 {
            points.extend(
                t.phrase_boundaries
                    .iter()
                    .filter(|p| p.confidence >= 0.5)
                    .map(|p| p.time_sec),
            );
        }
        if !(t.phrase_boundaries.is_empty() && t.sections.len() <= 1)
            && !points
                .iter()
                .any(|at| (grid.beat(*at) - grid.beat(anchor)).abs() <= grid.meter() * 0.5)
        {
            return false;
        }
    }
    true
}

/// Fit a release using measured note cycles/rests as well as structural edges.
/// Note evidence adds layer windows, never a whole-track phrase-cut permission.
pub(crate) fn windows(
    t: &TrackAnalysis,
    source: &Polyline,
    n: f32,
    desired: (f32, f32),
    outgoing: bool,
) -> Vec<(f32, f32)> {
    let (start, end) = (source.sample(desired.0), source.sample(desired.1));
    let grid = crate::grid::Grid(t);
    let lower = source.sample(0.);
    let upper = source.sample(n);
    let anchor = if outgoing { end } else { start };
    let mut anchors = edges(t);
    if !outgoing {
        // A gradual layer can be introduced on a metrical phrase entrance;
        // outgoing releases still require observed arrangement changes.
        anchors.extend(
            t.phrase_boundaries
                .iter()
                .filter(|p| p.confidence >= 0.5)
                .map(|p| p.time_sec),
        );
    }
    if t.phrase_boundaries.is_empty() && t.sections.len() <= 1 {
        anchors.push(anchor);
        let mut beat = grid.ceil_bar(lower);
        while grid.sec(beat) <= upper + 0.01 {
            anchors.push(grid.sec(beat));
            beat += grid.meter();
        }
    }
    let mut choices = Vec::new();
    if quiet(t, start, end) {
        choices.push((desired, 0.));
    }
    for at in anchors {
        for width in [4., 2., 1.25] {
            let beat = grid.beat(at);
            let (lo, hi) = if outgoing {
                (grid.sec(beat - width * grid.meter()), at)
            } else {
                (at, grid.sec(beat + width * grid.meter()))
            };
            if lo >= lower - 0.05 && hi <= upper + 0.05 && hi > lo {
                choices.push((
                    (coordinate(source, lo), coordinate(source, hi)),
                    (at - anchor).abs() + if width < 4. { 0.01 } else { 0. },
                ));
            }
        }
    }
    choices.sort_by(|a, b| a.1.total_cmp(&b.1));
    choices.into_iter().map(|p| p.0).collect()
}

/// Keep note proposals separate until after feasibility filtering. Adding a
/// nearby note rest must not evict a previously viable structural candidate
/// from the bounded search before either arrangement has been evaluated.
pub(crate) fn note_windows(
    t: &TrackAnalysis,
    source: &Polyline,
    n: f32,
    desired: (f32, f32),
    outgoing: bool,
) -> Vec<(f32, f32)> {
    let (lower, upper) = (source.sample(0.), source.sample(n));
    let anchor = source.sample(if outgoing { desired.1 } else { desired.0 });
    let mut choices: Vec<_> = notes::windows(t)
        .into_iter()
        .filter(|(lo, hi)| *lo >= lower - 0.001 && *hi <= upper + 0.001 && hi > lo)
        .collect();
    choices.sort_by(|a, b| {
        ((if outgoing { a.1 } else { a.0 }) - anchor)
            .abs()
            .total_cmp(&((if outgoing { b.1 } else { b.0 }) - anchor).abs())
    });
    choices
        .into_iter()
        .map(|(lo, hi)| (coordinate(source, lo), coordinate(source, hi)))
        .collect()
}

#[cfg(test)]
fn fit(
    t: &TrackAnalysis,
    source: &Polyline,
    n: f32,
    desired: (f32, f32),
    outgoing: bool,
) -> Option<(f32, f32)> {
    windows(t, source, n, desired, outgoing).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{PhraseBoundary, SectionLabel};
    #[test]
    fn synth_release_uses_observed_phrase_and_four_local_bars() {
        for bpm in [90., 120., 175.] {
            let mut t = crate::tests::track(1, bpm, "8A", SectionLabel::Outro, 64, 0.9, 0.2);
            let bar = 240. / bpm;
            t.phrase_boundaries = vec![
                PhraseBoundary {
                    time_sec: 16. * bar,
                    confidence: 0.9,
                    novelty: 0.5,
                },
                PhraseBoundary {
                    time_sec: 20. * bar,
                    confidence: 0.9,
                    novelty: 0.,
                },
            ];
            let source = Polyline {
                nodes: vec![(0., 0.), (32., 32. * bar)],
            };
            let release = fit(&t, &source, 32., (19.5, 20.), true).unwrap();
            assert!((release.0 - 12.).abs() < 0.01 && (release.1 - 16.).abs() < 0.01);
            let entry = fit(&t, &source, 32., (19.5, 20.), false).unwrap();
            assert!((entry.0 - 20.).abs() < 0.01 && (entry.1 - 24.).abs() < 0.01);
        }
    }
}
