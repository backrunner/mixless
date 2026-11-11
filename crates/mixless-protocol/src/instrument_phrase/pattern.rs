use super::{pulse, InstrumentBoundary, InstrumentBoundaryKind};
use crate::{StemNote, TrackAnalysis};

fn slice(notes: &[StemNote], start: f32, end: f32) -> &[StemNote] {
    let lo = notes.partition_point(|n| n.start_sec < start);
    let hi = notes.partition_point(|n| n.start_sec < end);
    &notes[lo..hi]
}

// Match polyphonic notes one-to-one: duplicated harmonics cannot explain every
// note in a different melody. Absolute pitches retain harmonic identity;
// transposing the whole recording preserves the result.
fn similarity(a: &[StemNote], b: &[StemNote], lag: f32, pulse: f32) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.;
    }
    let mut used = vec![false; b.len()];
    let mut matched = 0.;
    for n in a {
        let candidate = b
            .iter()
            .enumerate()
            .filter(|(i, m)| {
                !used[*i]
                    && m.midi == n.midi
                    && (m.start_sec - n.start_sec - lag).abs() <= pulse * 0.15
                    && ((m.end_sec - m.start_sec) - (n.end_sec - n.start_sec)).abs()
                        <= pulse * 0.25 + (n.end_sec - n.start_sec) * 0.15
            })
            .min_by(|(_, x), (_, y)| {
                (x.start_sec - n.start_sec - lag)
                    .abs()
                    .total_cmp(&(y.start_sec - n.start_sec - lag).abs())
            });
        if let Some((i, m)) = candidate {
            used[i] = true;
            matched += n.confidence.min(m.confidence);
        }
    }
    2. * matched / a.iter().chain(b).map(|n| n.confidence).sum::<f32>()
}

// Keep simultaneous notes together; do not turn a chord's arbitrary storage
// ordering into an ascending melody. This is a pitch-centroid contour, not a
// claim to have separated the lead from accompaniment.
fn contour(notes: &[StemNote], pulse: f32) -> Vec<(f32, f32)> {
    let mut groups: Vec<(f32, f32, f32)> = vec![];
    for n in notes {
        if let Some(g) = groups
            .last_mut()
            .filter(|g| n.start_sec - g.0 <= pulse * 0.15)
        {
            g.1 += n.midi as f32 * n.confidence;
            g.2 += n.confidence;
        } else {
            groups.push((n.start_sec, n.midi as f32 * n.confidence, n.confidence));
        }
    }
    groups.into_iter().map(|(s, p, w)| (s, p / w)).collect()
}
fn contour_similarity(a: &[StemNote], b: &[StemNote], lag: f32, pulse: f32) -> f32 {
    let (a, b) = (contour(a, pulse), contour(b, pulse));
    let mut pairs = vec![];
    let mut next = 0;
    for &(time, pitch) in &a {
        if let Some((index, &(_, other))) = b
            .iter()
            .enumerate()
            .skip(next)
            .filter(|(_, v)| (v.0 - time - lag).abs() <= pulse * 0.15)
            .min_by(|(_, x), (_, y)| {
                (x.0 - time - lag)
                    .abs()
                    .total_cmp(&(y.0 - time - lag).abs())
            })
        {
            pairs.push((pitch, other));
            next = index + 1;
        }
    }
    // A missing transcription onset should not erase an otherwise observed
    // contour. Compare intervals between matched attack groups, not indices.
    if pairs.len() < 3 || pairs.len() as f32 / (a.len().max(b.len()) as f32) < 0.85 {
        return 0.;
    }
    let mut movement = 0.;
    let mut error = 0.;
    for pair in pairs.windows(2) {
        let (x, y) = (pair[1].0 - pair[0].0, pair[1].1 - pair[0].1);
        movement += x.abs().max(y.abs());
        error += (x - y).abs();
    }
    // A repeated single held pitch, tremolo or stable pad is not a sequence
    // with a demonstrated melodic cycle ending.
    if movement < 2. {
        0.
    } else {
        (1. - error / movement).max(0.)
    }
}

/// A supporting voice can repeat a rhythmic pitch without melodic motion.
/// It cannot itself nominate a melodic ending, and a held crossing note still
/// prevents moving the shared instrument lane through this cycle.
pub(super) fn supports_cycle(t: &TrackAnalysis, notes: &[StemNote], at: f32, period: f32) -> bool {
    let Some(pulse) = pulse(t, at) else {
        return false;
    };
    let tolerance = pulse * 0.15;
    let a = slice(notes, at - 2. * period - tolerance, at - period - tolerance);
    let b = slice(notes, at - period - tolerance, at - tolerance);
    let c = slice(notes, at - tolerance, at + period - tolerance);
    !a.is_empty()
        && !b.is_empty()
        && !c.is_empty()
        && similarity(a, b, period, pulse).min(similarity(b, c, period, pulse)) >= 0.85
        && !notes
            .iter()
            .any(|n| n.start_sec < at - tolerance && n.end_sec > at + pulse * 0.25)
}

pub(super) fn boundaries(t: &TrackAnalysis, notes: &[StemNote]) -> Vec<InstrumentBoundary> {
    // Each of the three disjoint cycles below needs at least three notes.
    // Most separated trajectories are short fragments, so reject them before
    // repeatedly walking the complete track clock for every fragment.
    if notes.len() < 9 {
        return vec![];
    }
    let mut result = vec![];
    // Downbeats select the phase of a repeated motif. Recurrence alone is
    // equally true at every cyclic rotation and cannot locate its beginning.
    for (i, &at) in t.tempo.downbeats.iter().enumerate() {
        let Some(pulse) = pulse(t, at) else { continue };
        for bars in [1, 2, 4, 8] {
            if i < bars * 2 || i + bars >= t.tempo.downbeats.len() {
                continue;
            }
            let (a, b, c) = (
                t.tempo.downbeats[i - bars * 2],
                t.tempo.downbeats[i - bars],
                t.tempo.downbeats[i + bars],
            );
            let period = at - b;
            if period < 2. * pulse
                || period > 32. * pulse
                || ((b - a) / period - 1.).abs() > 0.05
                || ((c - at) / period - 1.).abs() > 0.05
            {
                continue;
            }
            let s = &t.tempo;
            let first = s.beats.partition_point(|v| *v < a);
            let last = s.beats.partition_point(|v| *v < c);
            if first >= last
                || last >= s.beats.len()
                || !(first..last).all(|beat| {
                    s.segments.iter().enumerate().any(|(j, seg)| {
                        beat as f32 >= seg.start_beat
                            && (beat as f32) < seg.end_beat
                            && s.pulse_confidence.get(j).copied().unwrap_or(seg.confidence) >= 0.65
                    })
                })
            {
                continue;
            }
            let tolerance = pulse * 0.15;
            let previous = slice(notes, a - tolerance, b - tolerance);
            let middle = slice(notes, b - tolerance, at - tolerance);
            let next = slice(notes, at - tolerance, c - tolerance);
            if middle.len() < 3 || previous.len() < 3 || next.len() < 3 {
                continue;
            }
            let similarity = similarity(previous, middle, b - a, pulse)
                .min(similarity(middle, next, period, pulse));
            let contour = contour_similarity(previous, middle, b - a, pulse)
                .min(contour_similarity(middle, next, period, pulse));
            if similarity < 0.85 || contour < 0.85 {
                continue;
            }
            // A new note group must actually begin on this cycle. Retain the
            // previous group's release separately, including its short tail.
            let attack = next[0].start_sec;
            let release = middle.iter().map(|n| n.end_sec).fold(0., f32::max);
            if (attack - at).abs() > tolerance
                || release > attack + 0.25 * pulse
                || notes
                    .iter()
                    .any(|n| n.start_sec < at - tolerance && n.end_sec > attack + 0.25 * pulse)
            {
                continue;
            }
            result.push(InstrumentBoundary {
                kind: InstrumentBoundaryKind::Motif,
                start_sec: middle[0].start_sec,
                release_sec: release,
                next_attack_sec: attack,
                period_sec: Some(period),
                note_similarity: similarity,
                contour_similarity: contour,
            });
            // Prefer the shortest demonstrated complete cycle. Longer spans
            // made of the same motif do not add distinct musical evidence.
            break;
        }
    }
    result
}
