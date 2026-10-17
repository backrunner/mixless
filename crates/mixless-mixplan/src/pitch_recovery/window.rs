//! Evidence gates for an intentional filtered solo retune. A breakdown label
//! alone does not establish a gap in the foreground.
use crate::grid::Grid;
use mixless_protocol::{SectionLabel as S, StemKind, TrackAnalysis};

#[derive(Clone, Copy)]
pub(super) struct Window {
    pub start: f32,
    pub end: f32,
    pub risk: f32,
}

pub(super) fn find(
    t: &TrackAnalysis,
    from: f32,
    until: f32,
    rate: f32,
    shift: f32,
) -> Option<Window> {
    let stems = t.stems.as_ref().filter(|s| s.valid(t.duration_sec))?;
    // `partial` also marks the analyzer's deterministic fallback path; it is
    // not a source-coverage flag. Require complete measured stems above and
    // assess the local grid/foreground below instead.
    if t.tempo.downbeats.len() < 2 || until <= from {
        return None;
    }
    let g = Grid(t);
    let mut best: Option<Window> = None;
    for &start in &t.tempo.downbeats {
        if start < from || start >= until {
            continue;
        }
        let beat = g.beat(start);
        if !t
            .tempo
            .segments
            .iter()
            .any(|s| beat >= s.start_beat && beat < s.end_beat && s.confidence >= 0.65)
        {
            continue;
        }
        for bars in [2., 4., 8.] {
            let end_beat = beat + bars * g.meter();
            let segments: Vec<_> = t
                .tempo
                .segments
                .iter()
                .filter(|s| s.start_beat < end_beat && s.end_beat > beat)
                .collect();
            if segments.first().is_none_or(|s| s.start_beat > beat + 0.001)
                || segments
                    .last()
                    .is_none_or(|s| s.end_beat < end_beat - 0.001)
                || segments.iter().any(|s| s.confidence < 0.65)
                || segments
                    .windows(2)
                    .any(|s| s[1].start_beat > s[0].end_beat + 0.001)
            {
                continue;
            }
            let end = g.sec(end_beat);
            let seconds = (end - start) / rate;
            // Quintic pitch movement occupies 60% of the gesture. Its peak
            // derivative is 1.875: cap the resulting speed at one semitone/s.
            // Also give the filter at least one wall second to close/open.
            if end > until - rate || seconds < 5. || seconds * 0.6 < shift * 1.875 {
                continue;
            }
            if !t.sections.iter().any(|s| {
                start >= s.start_sec
                    && end <= s.end_sec
                    && matches!(s.label, S::Intro | S::Break | S::Breakdown | S::Outro)
            }) {
                continue;
            }
            let first = stems.frames.partition_point(|f| f.end_sec <= start);
            let last = stems.frames.partition_point(|f| f.start_sec < end);
            let frames = &stems.frames[first..last];
            if frames.is_empty() {
                continue;
            }
            // A quiet vocal leak is harmless; an active foreground voice is
            // not. Ratio and level comparisons share the source's gain.
            if frames.iter().any(|f| {
                f.vocal_activity > 0.2 && f.rms[0] > 0.063 * (f.rms[1] + f.rms[2]).max(1e-8)
            }) {
                continue;
            }
            let move_start = start + (end - start) * 0.2;
            let move_end = start + (end - start) * 0.8;
            let two_beats = g.sec(beat + 2.) - start;
            // A held vocal is never a retuning window. A quiet instrumental
            // bed permits only a one-semitone move, four times slower than an
            // articulated passage; a filter is not proof that a note is gone.
            let held = |kind| {
                stems.notes.iter().any(|n| {
                    n.stem == kind
                        && n.confidence >= 0.6
                        && n.end_sec - n.start_sec >= two_beats
                        && n.start_sec < move_end
                        && n.end_sec > move_start
                })
            };
            if held(StemKind::Vocals) {
                continue;
            }
            let mut sustained = 0.;
            let mut held_note = false;
            for m in t
                .moments
                .iter()
                .filter(|m| m.start_sec < move_end && m.end_sec > move_start)
            {
                if m.pitch_midi.is_some() && m.sustain >= 0.7 {
                    sustained += m.end_sec.min(move_end) - m.start_sec.max(move_start);
                    held_note |= sustained >= two_beats;
                } else {
                    sustained = 0.;
                }
            }
            if (held_note || held(StemKind::Instruments) || held(StemKind::Bass))
                && (shift > 1. || seconds * 0.6 < shift * 7.5)
            {
                continue;
            }
            let mut context: Vec<_> = t
                .bars
                .iter()
                .filter(|b| b.start_sec < end + (end - start) && b.end_sec > start - (end - start))
                .map(|b| b.rms)
                .collect();
            if context.len() < 4 {
                continue;
            }
            context.sort_by(f32::total_cmp);
            let reference = context[context.len() * 3 / 4];
            let bars: Vec<_> = t
                .bars
                .iter()
                .filter(|b| b.start_sec < end && b.end_sec > start)
                .collect();
            if reference <= 1e-8
                || bars.first().is_none_or(|b| b.start_sec > start + 0.001)
                || bars.last().is_none_or(|b| b.end_sec < end - 0.001)
                || bars
                    .windows(2)
                    .any(|b| b[1].start_sec > b[0].end_sec + 0.001)
            {
                continue;
            }
            let mean = bars
                .iter()
                .map(|b| b.rms * (b.end_sec.min(end) - b.start_sec.max(start)))
                .sum::<f32>()
                / (end - start);
            // At least a 3 dB relaxation compared with the nearby louder
            // material. A bass mute inside a full-energy peak is not enough.
            if mean > reference * std::f32::consts::FRAC_1_SQRT_2 {
                continue;
            }
            let risk = mean / reference + 0.01 * seconds + 0.002 * (start - from) / rate;
            let candidate = Window { start, end, risk };
            if best.is_none_or(|old| risk < old.risk) {
                best = Some(candidate);
            }
        }
    }
    best
}
