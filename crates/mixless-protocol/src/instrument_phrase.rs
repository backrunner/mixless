//! Polyphonic note-group evidence for instrumental exchanges. A MIDI note is
//! not a melodic sentence, and overlapping notes need not be one voice.
//! Evidence for gradual shared-layer exchanges, never a whole-track hard cut.
use crate::{StemKind, StemNote, TrackAnalysis};
use serde::Serialize;

mod pattern;
mod scope;
pub use scope::{InstrumentEvidence, InstrumentScope};
mod voices;
pub use voices::{instrument_voices, InstrumentVoice};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentBoundaryKind {
    /// Notes release into a measured relative power retreat before a new attack.
    Rest,
    /// Pitch, timing and articulation recur across three complete cycles.
    Motif,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstrumentBoundary {
    pub kind: InstrumentBoundaryKind,
    /// Start of the completed note group/cycle, in original source seconds.
    pub start_sec: f32,
    /// Last release in that group; kept separate from the next onset.
    pub release_sec: f32,
    pub next_attack_sec: f32,
    pub period_sec: Option<f32>,
    pub note_similarity: f32,
    pub contour_similarity: f32,
}

/// Derived from the existing immutable analysis; no persisted labels or new
/// model inference. Missing notes, frames or clock produce no positive evidence.
pub fn instrument_boundaries(t: &TrackAnalysis) -> Vec<InstrumentBoundary> {
    scope::cached(t).unwrap_or_else(|| detect(t))
}

pub(super) fn pulse(t: &TrackAnalysis, sec: f32) -> Option<f32> {
    let beats = &t.tempo.beats;
    let i = beats.partition_point(|b| *b <= sec).saturating_sub(1);
    let mut bpm = beats
        .get(i + 1)
        .map_or(t.tempo.global_bpm, |b| 60. / (b - beats[i]));
    if !bpm.is_finite() || !(20. ..=400.).contains(&bpm) {
        return None;
    }
    while bpm < 80. {
        bpm *= 2.;
    }
    while bpm > 160. {
        bpm *= 0.5;
    }
    Some(60. / bpm)
}

fn notes(t: &TrackAnalysis) -> Vec<StemNote> {
    let Some(s) = &t.stems else { return vec![] };
    let mut result: Vec<StemNote> = vec![];
    let mut last = [None; 128];
    for note in s
        .notes
        .iter()
        .filter(|n| n.stem == StemKind::Instruments && n.confidence >= 0.5)
    {
        let Some(pulse) = pulse(t, note.start_sec) else {
            return vec![];
        };
        let i = note.midi as usize;
        // The decoder can fragment a held pitch at window/threshold edges.
        // Join only that pitch, and only without a new measured stem attack.
        if let Some(index) = last[i] {
            let prev: &mut StemNote = &mut result[index];
            if note.start_sec >= prev.start_sec
                && note.start_sec - prev.end_sec <= pulse * 0.1
                && s.frame_at(note.start_sec)
                    .is_some_and(|f| f.onset[2] < 0.25)
            {
                prev.end_sec = prev.end_sec.max(note.end_sec);
                prev.confidence = prev.confidence.min(note.confidence);
                continue;
            }
        }
        last[i] = Some(result.len());
        result.push(note.clone());
    }
    result
}

/// Time-weighted stem power with complete local coverage. A note-free interval
/// without acoustic support is a transcription hole, not an instrumental rest.
fn power(t: &TrackAnalysis, start: f32, end: f32) -> Option<f32> {
    if end <= start {
        return None;
    }
    let s = t.stems.as_ref()?;
    let first = s.frames.partition_point(|f| f.end_sec <= start);
    let (mut covered, mut sum) = (start, 0.);
    for f in s.frames[first..].iter().take_while(|f| f.start_sec < end) {
        if f.start_sec > covered + 0.001 || !f.rms[2].is_finite() {
            return None;
        }
        let dt = f.end_sec.min(end) - f.start_sec.max(start);
        sum += f.rms[2].powi(2) * dt;
        covered = f.end_sec;
    }
    (covered >= end - 0.001).then_some(sum / (end - start))
}

fn detect(t: &TrackAnalysis) -> Vec<InstrumentBoundary> {
    let Some(s) = &t.stems else { return vec![] };
    // Public callers may inspect old/incomplete analyses outside the planner.
    if !s.valid(t.duration_sec) {
        return vec![];
    }
    let notes = notes(t);
    if notes.len() < 3 {
        return vec![];
    }
    let mut result = vec![];
    let (mut start, mut end, mut count) = (notes[0].start_sec, notes[0].end_sec, 1);
    for n in &notes[1..] {
        let Some(pulse) = pulse(t, n.start_sec) else {
            continue;
        };
        if n.start_sec - end >= pulse * 0.5 {
            let before = power(t, (end - pulse).max(start), end);
            let rest = power(t, end, n.start_sec);
            if count >= 3
                && end - start >= pulse * 2.
                && before
                    .zip(rest)
                    .is_some_and(|(a, b)| a > 0. && b < a * 0.25)
            {
                result.push(InstrumentBoundary {
                    kind: InstrumentBoundaryKind::Rest,
                    start_sec: start,
                    release_sec: end,
                    next_attack_sec: n.start_sec,
                    period_sec: None,
                    note_similarity: 0.,
                    contour_similarity: 0.,
                });
            }
            start = n.start_sec;
            end = n.end_sec;
            count = 1;
        } else {
            end = end.max(n.end_sec);
            count += 1;
        }
    }
    // A dense bass pattern must not outvote a sparse evolving upper line in
    // one global similarity score. Follow each pitch/time trajectory and ask
    // every active voice to corroborate the proposed cycle independently.
    let voices = instrument_voices(t);
    let candidates = voices.iter().flat_map(|v| pattern::boundaries(t, &v.notes));
    for candidate in candidates {
        let period = candidate.period_sec.unwrap();
        let at = candidate.next_attack_sec;
        let active = voices.iter().filter(|v| {
            v.notes
                .iter()
                .any(|n| n.start_sec < at && n.end_sec > at - period)
        });
        if active.clone().all(|v| {
            // Ambiguous note assignment is retained in diagnostics, but does
            // not manufacture new permission to fade the shared lane.
            let local_notes = v
                .notes
                .iter()
                .filter(|n| n.start_sec >= at - period && n.start_sec < at)
                .count();
            let ambiguous = v
                .ambiguous_at_sec
                .iter()
                .filter(|s| **s >= at - period && **s < at)
                .count();
            ambiguous * 4 <= local_notes && pattern::supports_cycle(t, &v.notes, at, period)
        }) && !result.iter().any(|b| {
            b.kind == InstrumentBoundaryKind::Motif && (b.next_attack_sec - at).abs() < 0.05
        }) {
            let mut candidate = candidate;
            // The last held release of every participating voice matters.
            candidate.release_sec = active
                .flat_map(|v| &v.notes)
                .filter(|n| n.start_sec < at - pulse(t, at).unwrap_or(0.5) * 0.15)
                .map(|n| n.end_sec)
                .fold(candidate.release_sec, f32::max);
            result.push(candidate);
        }
    }
    result.sort_by(|a, b| a.release_sec.total_cmp(&b.release_sec));
    result
}

#[cfg(test)]
mod tests;
