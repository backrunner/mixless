//! Small gestures on the outgoing deck before the transition starts, so an
//! unattended AutoMix pass still sounds played: a filter riser into each real
//! drop and a restrained drum dip on measured phrase boundaries inside long drops.
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
    /// Section-local tonal attenuation in dB; neutral 0.
    Eq(mixless_protocol::EqBand),
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

// At Subtle the high-pass corner reaches 90 Hz (bass shading); Active can
// reach 180 Hz. Neither removes the whole low midrange as the old 400 Hz sweep did.
fn riser_depth(level: LiveMoves) -> f32 {
    let cutoff: f32 = if level == LiveMoves::Active {
        180.
    } else {
        90.
    };
    (cutoff / 30.).ln() / 600f32.ln()
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

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
    // Never clip a gesture: it must start after the current playhead and land
    // fully before the transition's own automation begins.
    let fits = |start: f32, end: f32| {
        start >= from_sec + 0.5
            && end <= until_sec - 60. / grid.bpm(grid.beat(end))
            && start < end
            && crate::smooth::grid_reliable(t, start, end)
    };
    let mut moves = filter_risers(t, &grid, level, &fits);
    if stems_ready {
        moves.extend(drum_pulls(t, &grid, level, &fits));
    }
    moves.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    // One gesture per lane at a time; a crowded boundary keeps the earlier move.
    // Leave a phrase of neutral playback between gestures, across all lanes.
    // Layered filter + drum moves can otherwise become an unintended bass cut.
    let mut next = f32::NEG_INFINITY;
    moves.retain(|m| {
        if m.start_sec < next {
            return false;
        }
        next = m.end_sec + 8. * bar_seconds(t, m.end_sec);
        true
    });
    moves.extend(crate::eq::solo(t, from_sec, until_sec));
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

/// Require continuous measurements; absence is not permission for an accent.
fn covered(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    let mut cursor = start;
    for b in t
        .bars
        .iter()
        .filter(|b| b.start_sec < end && b.end_sec > start)
    {
        if b.start_sec > cursor + 0.01 || b.rms <= 0.001 {
            return false;
        }
        cursor = b.end_sec;
    }
    cursor >= end - 0.01
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
        // The section name is not independent evidence for a filter move.
        if !mixless_protocol::has_buildup(&t.bars, section.start_sec, section.end_sec) {
            continue;
        }
        let end_beat = grid.beat(p0);
        let start_beat = (end_beat - sweep_beats).max(grid.beat(section.start_sec));
        let start = grid.sec(start_beat);
        let rise_end = grid.sec(end_beat - 1.);
        if rise_end <= start + 0.05 {
            continue;
        }
        if !covered(t, start, p0) {
            continue;
        }
        let vocal = bars_inside(t, start, p0)
            .iter()
            .map(|b| crate::vocals::risk(b))
            .fold(0f32, f32::max);
        // Sustained vocals get at most half of the already restrained move.
        let depth = riser_depth(level) * if vocal >= 0.45 { 0.5 } else { 1. };
        // Recover through the last beat, reaching neutral on the downbeat;
        // do not snap a 400 Hz high-pass open in 25 ms.
        let mut nodes = Vec::new();
        for k in 0..=32 {
            let f = k as f32 / 32.;
            nodes.push((start + (rise_end - start) * f, depth * ease(f)));
        }
        for k in 1..=16 {
            let f = k as f32 / 16.;
            nodes.push((rise_end + (p0 - rise_end) * f, depth * (1. - ease(f))));
        }
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

/// Shade the last beat of a measured phrase, then return on its downbeat.
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
            let start = grid.sec(boundary_beat - 1.);
            let measured_phrase = t
                .phrase_boundaries
                .iter()
                .any(|p| p.confidence >= 0.6 && (p.time_sec - b).abs() < 0.08);
            let clean = covered(t, start, b + 0.5 * bar)
                && t.bars
                    .iter()
                    .filter(|x| x.start_sec < b + 0.5 * bar && x.end_sec > start)
                    .all(|x| crate::vocals::risk(x) < 0.35);
            if driven && measured_phrase && clean && fits(start, b) {
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
                    // -3 dB in Subtle, -6 dB in Active; never an automatic mute.
                    nodes: (0..=32)
                        .map(|i| {
                            let phase = i as f32 / 32.;
                            let db = if level == LiveMoves::Active { -6. } else { -3. };
                            let depth = if phase < 0.5 {
                                ease(phase * 2.)
                            } else {
                                ease((1. - phase) * 2.)
                            };
                            (start + (b - start) * phase, 10f32.powf(db * depth / 20.))
                        })
                        .collect(),
                },
                label: "Drum dip",
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
            b.vocal_presence = 0.1;
            b.vocal_confidence = Some(0.1);
            if (16..32).contains(&b.bar_index) {
                b.rms = 0.1 + (b.bar_index - 16) as f32 * 0.02;
                b.onset_density = 0.1 + (b.bar_index - 16) as f32 * 0.06;
            }
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
        assert!(
            max > 0.16 && max <= riser_depth(LiveMoves::Subtle) + 0.001,
            "depth {max}"
        );
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
        for b in &mut t.bars {
            b.vocal_presence = 0.1;
            b.vocal_confidence = Some(0.1);
        }
        t.phrase_boundaries = [8., 16., 24.]
            .into_iter()
            .map(|b| mixless_protocol::PhraseBoundary {
                time_sec: b * bar,
                confidence: 0.9,
                novelty: 0.5,
            })
            .collect();
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
        assert_eq!(ends, vec![16.], "{active:?}");
        // No stems attached to the deck: the drum lane stays untouched.
        assert!(performance_moves(&t, 0., 70., false, LiveMoves::Active).is_empty());
        let mut sung = t.clone();
        for b in &mut sung.bars {
            b.vocal_confidence = Some(0.8);
        }
        assert!(performance_moves(&sung, 0., 70., true, LiveMoves::Active).is_empty());
        t.phrase_boundaries.clear();
        assert!(performance_moves(&t, 0., 70., true, LiveMoves::Active).is_empty());
    }

    #[test]
    fn labels_missing_coverage_and_a_lone_final_impact_do_not_authorize_risers() {
        let mut t = build_to_drop();
        for b in &mut t.bars {
            b.rms = 0.2;
            b.onset_density = 0.2;
        }
        assert!(performance_moves(&t, 0., 200., false, LiveMoves::Active).is_empty());
        t.bars[31].rms = 0.8;
        t.bars[31].onset_density = 0.9;
        assert!(performance_moves(&t, 0., 200., false, LiveMoves::Active).is_empty());
        let mut t = build_to_drop();
        t.bars.retain(|b| b.bar_index != 30);
        assert!(performance_moves(&t, 0., 200., false, LiveMoves::Active).is_empty());
    }

    #[test]
    fn measured_gain_time_shift_and_tempo_scaling_preserve_the_gesture() {
        let base = build_to_drop();
        let reference = performance_moves(&base, 0., 200., false, LiveMoves::Subtle);
        for scale in [0.75, 1., 1.25] {
            for gain in [0.5f32, 2.] {
                let shift = 9.;
                let mut t = base.clone();
                t.duration_sec = t.duration_sec * scale + shift;
                t.tempo.global_bpm /= scale;
                for segment in &mut t.tempo.segments {
                    segment.bpm /= scale;
                }
                for time in t.tempo.beats.iter_mut().chain(&mut t.tempo.downbeats) {
                    *time = *time * scale + shift;
                }
                for section in &mut t.sections {
                    section.start_sec = section.start_sec * scale + shift;
                    section.end_sec = section.end_sec * scale + shift;
                }
                for b in &mut t.bars {
                    b.start_sec = b.start_sec * scale + shift;
                    b.end_sec = b.end_sec * scale + shift;
                    b.rms *= gain;
                    for db in [&mut b.low_db, &mut b.mid_db, &mut b.high_db] {
                        *db += 20. * gain.log10();
                    }
                }
                let result =
                    performance_moves(&t, shift, 200. * scale + shift, false, LiveMoves::Subtle);
                assert_eq!(result.len(), reference.len());
                for (actual, expected) in result.iter().zip(&reference) {
                    assert!((actual.start_sec - (expected.start_sec * scale + shift)).abs() < 0.01);
                    assert!((actual.end_sec - (expected.end_sec * scale + shift)).abs() < 0.01);
                    let peak = |m: &PerformanceMove| {
                        m.curve.nodes.iter().map(|p| p.1).fold(0f32, f32::max)
                    };
                    assert!((peak(actual) - peak(expected)).abs() < 0.001);
                }
            }
        }
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
