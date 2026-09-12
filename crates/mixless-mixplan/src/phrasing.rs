use crate::{constraints::user_range, grid::Grid};
use mixless_protocol::{Cue, CueKind, TrackAnalysis};

pub(crate) fn boundary_quality(t: &TrackAnalysis, sec: f32) -> f32 {
    if !t.phrase_boundaries.is_empty() {
        return t
            .phrase_boundaries
            .iter()
            .filter(|p| (p.time_sec - sec).abs() < 0.08)
            .map(|p| (p.confidence + p.novelty.min(1.) * 0.1).min(1.))
            .fold(0., f32::max);
    }
    // Legacy/hand-annotated analyses: infer metrical phrases relative to a
    // section's first downbeat, never from the bar feature's array index.
    let g = Grid(t);
    t.sections
        .iter()
        .map(|s| {
            if (s.start_sec - sec).abs() < 0.08 || (s.end_sec - sec).abs() < 0.08 {
                return 0.75;
            }
            if sec < s.start_sec || sec > s.end_sec {
                return 0.;
            }
            let bars = (g.beat(sec) - g.ceil_bar(s.start_sec)) / g.meter();
            if (bars / 8. - (bars / 8.).round()).abs() < 0.01 {
                0.6
            } else {
                0.
            }
        })
        .fold(0., f32::max)
}

pub(crate) fn points(t: &TrackAnalysis, cues: &[Cue], out: bool, earliest: f32) -> Vec<f32> {
    let g = Grid(t);
    let explicit: Vec<_> = cues
        .iter()
        .filter(|c| c.user_set && c.kind != CueKind::Hot)
        .collect();
    let role = if out { CueKind::Out } else { CueKind::In };
    if explicit.len() == 1 && explicit[0].kind == role {
        let sec = explicit[0].frame as f32 / t.sample_rate.max(1) as f32;
        let reliable =
            t.tempo.beats.len() >= 8 && t.tempo.segments.iter().any(|s| s.confidence >= 0.65);
        let snapped = if out {
            g.ceil_bar(sec)
        } else {
            g.floor_bar(sec)
        };
        // An uncertain grid (or a final pickup beyond its last full bar)
        // cannot move a manual marker outside the audio. Use a native bridge.
        return vec![if reliable && g.sec(snapped) <= t.duration_sec {
            snapped
        } else {
            g.beat(sec)
        }];
    }
    let audible_end = t
        .bars
        .iter()
        .rev()
        .find(|b| b.rms > 0.001)
        .map_or(t.duration_sec, |b| b.end_sec.min(t.duration_sec));
    let mut result: Vec<_> = t
        .phrase_boundaries
        .iter()
        .filter(|p| p.confidence >= 0.5)
        .map(|p| g.beat(p.time_sec))
        .collect();
    let kind = if out {
        mixless_protocol::MixRegionKind::Out
    } else {
        mixless_protocol::MixRegionKind::In
    };
    result.extend(
        t.mix_regions
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| g.beat(r.anchor_sec)),
    );
    for s in &t.sections {
        if out {
            result.push(g.floor_bar(s.end_sec));
        } else {
            result.push(g.ceil_bar(s.start_sec));
        }
        if t.phrase_boundaries.is_empty() {
            let mut beat = g.ceil_bar(s.start_sec);
            while g.sec(beat) < s.end_sec {
                result.push(beat);
                beat += 8. * g.meter();
            }
        }
    }
    if out {
        result.push(g.floor_bar(audible_end));
    } else {
        result.push(
            g.ceil_bar(
                t.bars
                    .iter()
                    .find(|b| b.rms > 0.001)
                    .map_or(0., |b| b.start_sec),
            ),
        );
    }
    let manual = crate::cue_policy::auto_anchors(t, cues, out);
    result.extend(&manual);
    result.retain(|b| {
        let is_manual = manual.iter().any(|m| (*m - *b).abs() < 0.01);
        let sec = g.sec(*b);
        b.is_finite()
            && sec >= 0.
            && sec <= audible_end + 0.001
            && if out {
                sec + 0.001 >= earliest + g.meter() * 60. / g.bpm(*b)
                    && (is_manual || sec > t.duration_sec * 0.45)
            } else {
                is_manual || sec < t.duration_sec * 0.5
            }
    });
    let merit = |beat: f32| {
        let sec = g.sec(beat);
        // Entry checks the incoming phrase, not the previous one.
        let probe = if out { sec - 0.01 } else { sec + 0.01 };
        let bar = t
            .bars
            .iter()
            .find(|b| probe >= b.start_sec && probe < b.end_sec);
        let vocal = bar.map_or(0.6, |b| b.vocal_presence);
        let silence = bar.is_some_and(|b| b.rms <= 0.001);
        let position = if out {
            sec / t.duration_sec
        } else {
            1. - sec / t.duration_sec
        };
        let hot = cues.iter().any(|c| {
            c.kind == CueKind::Hot
                && c.user_set
                && (c.frame as f32 / t.sample_rate as f32 - sec).abs()
                    <= 2. * g.meter() * 60. / g.bpm(beat)
        });
        let window = t
            .mix_regions
            .iter()
            .filter(|r| r.kind == kind && (r.anchor_sec - sec).abs() < 0.15)
            .map(|r| r.confidence)
            .fold(0., f32::max);
        boundary_quality(t, sec).max(crate::cue_policy::manual_quality(t, cues, sec, out)) * 0.8
            + window * 0.4
            + position * 0.3
            - vocal * 0.5
            + if hot { 0.3 } else { 0. }
            - if silence { 2. } else { 0. }
    };
    result.sort_by(|a, b| merit(*b).total_cmp(&merit(*a)).then(a.total_cmp(b)));
    if let Some((start, end)) = user_range(cues, t.sample_rate) {
        result.insert(
            0,
            if out {
                g.ceil_bar(end)
            } else {
                g.floor_bar(start)
            },
        );
    }
    let mut unique = Vec::new();
    for beat in result {
        if !unique.iter().any(|b: &f32| (*b - beat).abs() < 0.01) {
            unique.push(beat);
        }
    }
    unique.truncate(8);
    unique
}
