//! Small gestures on the outgoing deck before the transition starts, so an
//! unattended AutoMix pass still sounds played: a filter riser into each real
//! drop and a one-beat drum pull-out on phrase boundaries inside long drops.
//! Every curve starts and ends on the lane's neutral value; the host owns the
//! playhead, takeover detection and neutral restoration.
use crate::grid::Grid;
use mixless_protocol::{Polyline, SectionLabel as S, StemKind, TrackAnalysis};

pub use mixless_protocol::LiveMoves;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PerformanceLane {
    /// Channel filter amount; neutral 0, positive sweeps high-pass.
    Filter,
    /// Stem gain 0..=1; neutral 1.
    Stem(StemKind),
}

#[derive(Debug, Clone)]
pub struct PerformanceMove {
    pub lane: PerformanceLane,
    /// Source seconds on the outgoing deck; rate changes do not shift them.
    pub start_sec: f32,
    pub end_sec: f32,
    /// (source seconds, value) nodes sampled with `Polyline::sample`.
    pub curve: Polyline,
    pub label: &'static str,
}

// Engine `ChannelFilter::cutoff` maps a positive amount to a high-pass corner
// of 30 Hz * 600^amount (the 18 kHz upper clamp is below sr*0.45 at every real
// rate). ln(400/30) / ln(600) puts the riser's resting corner near 400 Hz.
const RISER_DEPTH: f32 = 0.405;

pub fn performance_moves(
    t: &TrackAnalysis,
    from_sec: f32,
    until_sec: f32,
    stems_ready: bool,
    level: LiveMoves,
) -> Vec<PerformanceMove> {
    if level == LiveMoves::Off {
        return Vec::new();
    }
    let grid = Grid(t);
    let beat = 60. / t.tempo.global_bpm.max(20.);
    // Never clip a gesture: it must start after the current playhead and land
    // fully before the transition's own automation begins.
    let fits = |start: f32, end: f32| {
        start >= from_sec + 0.5
            && end <= until_sec - beat
            && start < end
            && crate::smooth::grid_reliable(t, start, end)
    };
    let mut moves = filter_risers(t, &grid, level, &fits);
    if stems_ready {
        moves.extend(drum_pulls(t, &grid, level, &fits));
    }
    moves.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    // One gesture per lane at a time; a crowded boundary keeps the earlier move.
    let mut ends = std::collections::HashMap::new();
    moves.retain(|m| match ends.get(&m.lane) {
        Some(&end) if m.start_sec < end - 0.001 => false,
        _ => {
            ends.insert(m.lane, m.end_sec);
            true
        }
    });
    moves
}

fn bar_seconds(t: &TrackAnalysis, at: f32) -> f32 {
    let grid = Grid(t);
    grid.meter() * 60. / grid.bpm(grid.beat(at))
}

/// Bars whose span falls inside [start, end], for section-local evidence.
fn bars_inside<'a>(
    t: &'a TrackAnalysis,
    start: f32,
    end: f32,
) -> Vec<&'a mixless_protocol::BarFeature> {
    t.bars
        .iter()
        .filter(|b| b.start_sec >= start - 0.01 && b.end_sec <= end + 0.01)
        .collect()
}

fn mean(
    bars: &[&mixless_protocol::BarFeature],
    f: impl Fn(&mixless_protocol::BarFeature) -> f32,
) -> f32 {
    if bars.is_empty() {
        0.
    } else {
        bars.iter().map(|b| f(b)).sum::<f32>() / bars.len() as f32
    }
}

/// A long build or a quiet break releases tension into the peak; riding the
/// filter up over its last bars is the standard booth gesture.
fn filter_risers(
    t: &TrackAnalysis,
    grid: &Grid,
    level: LiveMoves,
    fits: &impl Fn(f32, f32) -> bool,
) -> Vec<PerformanceMove> {
    let meter = grid.meter();
    let sweep_beats = meter * if level == LiveMoves::Active { 8. } else { 4. };
    let mut moves = Vec::new();
    for &(p0, _) in &crate::drops::peaks(t) {
        let Some(section) = t.sections.iter().find(|s| (s.end_sec - p0).abs() < 0.08) else {
            continue;
        };
        if matches!(section.label, S::Drop | S::Chorus | S::Silence) {
            continue;
        }
        let bar = bar_seconds(t, section.end_sec - 0.01);
        let section_len = section.end_sec - section.start_sec;
        if section_len < 4. * bar - 0.05 {
            continue;
        }
        let inside = bars_inside(t, section.start_sec, section.end_sec);
        let rising_onsets = inside.len() >= 4 && {
            let first = mean(&inside[..4], |b| b.onset_density);
            let last = mean(&inside[inside.len() - 4..], |b| b.onset_density);
            last >= first * 1.2 && last >= 0.1
        };
        let builds = section.label == S::BuildUp
            || rising_onsets
            || mixless_protocol::has_buildup(&t.bars, section.start_sec, section.end_sec);
        if !builds {
            continue;
        }
        let end_beat = grid.beat(p0);
        let start_beat = (end_beat - sweep_beats).max(grid.beat(section.start_sec));
        let start = grid.sec(start_beat);
        let rise_end = grid.sec(end_beat - 1.);
        if rise_end <= start + 0.05 {
            continue;
        }
        let depth = if mean(&bars_inside(t, start, p0), |b| b.vocal_presence) > 0.6 {
            RISER_DEPTH * 0.6
        } else {
            RISER_DEPTH
        };
        // A slow t^3 climb keeps the first bars imperceptible, then the corner
        // holds through the last beat and snaps open on the downbeat.
        let mut nodes = vec![(start, 0.)];
        for k in 1..9 {
            let f = k as f32 / 9.;
            nodes.push((start + (rise_end - start) * f, depth * f * f * f));
        }
        nodes.push((rise_end, depth));
        nodes.push((p0 - 0.03, depth));
        nodes.push((p0 - 0.005, 0.));
        if !fits(start, p0) {
            continue;
        }
        moves.push(PerformanceMove {
            lane: PerformanceLane::Filter,
            start_sec: start,
            end_sec: p0,
            curve: Polyline { nodes },
            label: "Filter riser",
        });
    }
    moves
}

/// Pull the drums for the final beat of a phrase inside a long drop; the kick
/// slamming back on the next eight-bar boundary reads as a live accent.
fn drum_pulls(
    t: &TrackAnalysis,
    grid: &Grid,
    level: LiveMoves,
    fits: &impl Fn(f32, f32) -> bool,
) -> Vec<PerformanceMove> {
    let meter = grid.meter();
    let mut moves = Vec::new();
    for &(p0, p1) in &crate::drops::major_peaks(t) {
        let p0_beat = grid.beat(p0);
        let last_beat = grid.beat(p1) - 4. * meter;
        if last_beat < p0_beat + 16. * meter - 0.01 {
            continue;
        }
        let bar = bar_seconds(t, p0 + 0.01);
        let bar_at = |sec: f32| {
            t.bars
                .iter()
                .find(|b| b.start_sec <= sec && sec < b.end_sec)
        };
        let mut k = 1.;
        let mut boundaries = Vec::new();
        loop {
            let boundary_beat = p0_beat + 8. * k * meter;
            if boundary_beat > last_beat + 0.01 {
                break;
            }
            let b = grid.sec(boundary_beat);
            let driven = bar_at(b - 0.5 * bar).is_some_and(|x| x.kick_salience >= 0.6)
                && bar_at(b + 0.5 * bar).is_some_and(|x| x.kick_salience >= 0.6);
            if driven && fits(grid.sec(boundary_beat - 1.), b) {
                boundaries.push((boundary_beat, b));
            }
            k += 1.;
        }
        let boundaries = if level == LiveMoves::Subtle {
            boundaries.last().copied().into_iter().collect()
        } else {
            boundaries
        };
        for (beat_at, b) in boundaries {
            let start = grid.sec(beat_at - 1.);
            moves.push(PerformanceMove {
                lane: PerformanceLane::Stem(StemKind::Drums),
                start_sec: start,
                end_sec: b,
                curve: Polyline {
                    nodes: vec![
                        (start, 1.),
                        (start + 0.02, 0.),
                        (b - 0.03, 0.),
                        (b - 0.005, 1.),
                    ],
                },
                label: "Drum drop-out",
            });
        }
    }
    moves
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::track;
    use mixless_protocol::Section;

    fn build_to_drop() -> TrackAnalysis {
        let mut t = track(1, 128., "8A", S::Intro, 96, 0.8, 0.6);
        let bar = 240. / 128.;
        t.sections = [
            (0., 16., S::Intro),
            (16., 32., S::BuildUp),
            (32., 64., S::Drop),
            (64., 80., S::Break),
            (80., 96., S::Drop),
        ]
        .iter()
        .map(|&(start, end, label)| Section {
            start_sec: start * bar,
            end_sec: end * bar,
            label,
        })
        .collect();
        for b in &mut t.bars {
            b.section = t
                .sections
                .iter()
                .find(|s| s.start_sec <= b.start_sec + 0.01 && b.end_sec <= s.end_sec + 0.01)
                .map_or(S::Unknown, |s| s.label);
        }
        t
    }

    #[test]
    fn filter_riser_lands_on_a_real_build_to_drop_boundary() {
        let t = build_to_drop();
        let moves = performance_moves(&t, 0., 200., false, LiveMoves::Subtle);
        assert_eq!(moves.len(), 1, "{moves:?}");
        let m = &moves[0];
        assert_eq!(m.lane, PerformanceLane::Filter);
        let bar = 240. / 128.;
        assert!((m.end_sec - 32. * bar).abs() < 0.05);
        assert!((m.start_sec - 28. * bar).abs() < 0.05);
        assert_eq!(m.curve.sample(m.start_sec), 0.);
        assert_eq!(m.curve.sample(m.end_sec), 0.);
        let max = (0..=100)
            .map(|i| {
                m.curve
                    .sample(m.start_sec + (m.end_sec - m.start_sec) * i as f32 / 100.)
            })
            .fold(0f32, f32::max);
        assert!(max >= 0.3, "depth {max}");
        // The Active level sweeps the build's last eight bars; the plain Break
        // before the second drop shows no build evidence and gets nothing.
        let active = performance_moves(&t, 0., 200., false, LiveMoves::Active);
        assert_eq!(active.len(), 1, "{active:?}");
        assert!((active[0].start_sec - 24. * bar).abs() < 0.05);
    }

    #[test]
    fn a_riser_that_would_not_finish_is_dropped_not_clipped() {
        let t = build_to_drop();
        let moves = performance_moves(&t, 0., 59., false, LiveMoves::Subtle);
        assert!(moves.is_empty(), "{moves:?}");
    }

    #[test]
    fn drum_pulls_hit_phrase_boundaries_inside_a_long_drop() {
        let mut t = track(1, 128., "8A", S::Drop, 40, 0.9, 0.8);
        let bar = 240. / 128.;
        t.sections = vec![
            Section {
                start_sec: 0.,
                end_sec: 32. * bar,
                label: S::Drop,
            },
            Section {
                start_sec: 32. * bar,
                end_sec: 40. * bar,
                label: S::Outro,
            },
        ];
        // A weak bar before the 8-bar boundary drops that candidate, leaving
        // the 16- and 24-bar accents.
        t.bars[7].kick_salience = 0.4;
        let subtle = performance_moves(&t, 0., 70., true, LiveMoves::Subtle);
        assert_eq!(subtle.len(), 1, "{subtle:?}");
        assert!(
            (subtle[0].end_sec - 24. * bar).abs() < 0.1,
            "{:?}",
            subtle[0]
        );
        assert_eq!(subtle[0].lane, PerformanceLane::Stem(StemKind::Drums));
        assert_eq!(subtle[0].curve.sample(subtle[0].start_sec), 1.);
        assert_eq!(subtle[0].curve.sample(subtle[0].end_sec), 1.);
        let active = performance_moves(&t, 0., 70., true, LiveMoves::Active);
        let ends: Vec<f32> = active.iter().map(|m| m.end_sec / bar).collect();
        assert_eq!(ends, vec![16., 24.], "{active:?}");
        // No stems attached to the deck: the drum lane stays untouched.
        assert!(performance_moves(&t, 0., 70., false, LiveMoves::Active).is_empty());
    }

    #[test]
    fn an_unreliable_grid_gets_no_moves() {
        let mut t = build_to_drop();
        t.tempo.pulse_confidence = vec![0.4];
        t.tempo.segments[0].confidence = 0.4;
        assert!(performance_moves(&t, 0., 200., true, LiveMoves::Active).is_empty());
        assert!(performance_moves(&t, 0., 200., true, LiveMoves::Off).is_empty());
    }
}
