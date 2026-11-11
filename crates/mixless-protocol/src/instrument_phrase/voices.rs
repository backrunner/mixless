//! Deterministic pitch/time voice leading, not instrument or lead identification.
//! Simultaneous notes compete in one assignment, so a low ostinato cannot absorb
//! an upper line merely because its note happens to appear first in storage.
use super::{notes, pulse};
use crate::{StemNote, TrackAnalysis};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct InstrumentVoice {
    pub notes: Vec<StemNote>,
    /// Duration/confidence-weighted pitch centre, in MIDI semitones.
    pub register: f32,
    /// Links with a competing assignment within one semitone's cost.
    pub ambiguous_links: usize,
    pub ambiguous_at_sec: Vec<f32>,
}

/// Group existing notes into monophonic trajectories. Register is relative
/// evidence, not a claim that the highest voice is always the melody.
pub fn instrument_voices(t: &TrackAnalysis) -> Vec<InstrumentVoice> {
    let Some(stems) = &t.stems else { return vec![] };
    if !stems.valid(t.duration_sec) {
        return vec![];
    }
    let notes = notes(t);
    let mut voices: Vec<InstrumentVoice> = vec![];
    let mut start = 0;
    while start < notes.len() {
        let at = notes[start].start_sec;
        let Some(pulse) = pulse(t, at) else { break };
        // 1/16 pulse absorbs small transcription onset disagreement, but
        // does not combine a normal arpeggio into one chord attack.
        let end = start + notes[start..].partition_point(|n| n.start_sec <= at + pulse / 16.);
        let mut group: Vec<_> = notes[start..end].iter().collect();
        group.sort_by(|a, b| {
            a.midi
                .cmp(&b.midi)
                .then(a.start_sec.total_cmp(&b.start_sec))
        });
        let active: Vec<_> = voices
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let last = v.notes.last()?;
                // Keep a voice across a short rest; a long disappearance starts
                // a new trajectory rather than borrowing an unrelated old pitch.
                (at - last.end_sec <= pulse * 8.).then_some(i)
            })
            .collect();
        let cost: Vec<Vec<f32>> = group
            .iter()
            .map(|note| {
                let mut row: Vec<_> = active
                    .iter()
                    .map(|&i| link(&voices[i], note, pulse))
                    .collect();
                // Each new note may start a voice, including during held chords.
                row.extend(std::iter::repeat_n(1.5, group.len()));
                row
            })
            .collect();
        let assigned = assignment(&cost);
        for (i, (&column, note)) in assigned.iter().zip(group).enumerate() {
            if column < active.len() && cost[i][column] < 1.5 {
                let alternative = cost[i]
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != column)
                    .map(|(_, x)| *x)
                    .fold(f32::INFINITY, f32::min);
                let v = &mut voices[active[column]];
                if alternative - cost[i][column] < 1. / 12. {
                    v.ambiguous_links += 1;
                    v.ambiguous_at_sec.push(note.start_sec);
                }
                v.notes.push(note.clone());
            } else {
                voices.push(InstrumentVoice {
                    notes: vec![note.clone()],
                    register: 0.,
                    ambiguous_links: 0,
                    ambiguous_at_sec: vec![],
                });
            }
        }
        start = end;
    }
    for v in &mut voices {
        let (sum, weight) = v.notes.iter().fold((0., 0.), |(sum, weight), n| {
            let w = (n.end_sec - n.start_sec) * n.confidence;
            (sum + n.midi as f32 * w, weight + w)
        });
        v.register = sum / weight.max(1e-6);
    }
    voices
}

fn link(v: &InstrumentVoice, n: &StemNote, pulse: f32) -> f32 {
    let last = v.notes.last().unwrap();
    let overlap = last.end_sec - n.start_sec;
    // Short release tails may overlap. Held notes remain an independent voice.
    if overlap > pulse / 4. || n.start_sec <= last.start_sec + pulse / 16. {
        return 1e6;
    }
    let interval = n.midi as f32 - last.midi as f32;
    let trend = v.notes.iter().rev().nth(1).map_or(0., |previous| {
        let delta = last.midi as f32 - previous.midi as f32;
        // A stable moving line can cross another voice; pitch ordering is not
        // hard coded. Limit extrapolation to an octave, and keep proximity first.
        (interval - delta.clamp(-12., 12.)).abs() / 24.
    });
    interval.abs() / 12. + (n.start_sec - last.end_sec).max(0.) / (8. * pulse) + trend
}

/// Minimum-cost rectangular assignment (rows <= columns). One predecessor per
/// note and one successor per voice; deterministic equal-cost ties use columns.
fn assignment(cost: &[Vec<f32>]) -> Vec<usize> {
    let (n, m) = (cost.len(), cost[0].len());
    let (mut u, mut v) = (vec![0.; n + 1], vec![0.; m + 1]);
    let (mut p, mut way) = (vec![0; m + 1], vec![0; m + 1]);
    for i in 1..=n {
        p[0] = i;
        let mut j = 0;
        let (mut minimum, mut used) = (vec![f32::INFINITY; m + 1], vec![false; m + 1]);
        loop {
            used[j] = true;
            let (mut delta, mut next) = (f32::INFINITY, 0);
            for k in 1..=m {
                if used[k] {
                    continue;
                }
                let reduced = cost[p[j] - 1][k - 1] - u[p[j]] - v[k];
                if reduced < minimum[k] {
                    minimum[k] = reduced;
                    way[k] = j;
                }
                if minimum[k] < delta {
                    delta = minimum[k];
                    next = k;
                }
            }
            for k in 0..=m {
                if used[k] {
                    u[p[k]] += delta;
                    v[k] -= delta;
                } else {
                    minimum[k] -= delta;
                }
            }
            j = next;
            if p[j] == 0 {
                break;
            }
        }
        loop {
            let previous = way[j];
            p[j] = p[previous];
            j = previous;
            if j == 0 {
                break;
            }
        }
    }
    let mut result = vec![0; n];
    for j in 1..=m {
        if p[j] != 0 {
            result[p[j] - 1] = j - 1;
        }
    }
    result
}

#[cfg(test)]
mod tests;
