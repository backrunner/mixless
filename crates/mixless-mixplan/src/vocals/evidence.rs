//! Distinguish an exposed vocal from an uncertain separator/note estimate.
use mixless_protocol::{StemFrame, StemKind, StemNote, TrackAnalysis};
use std::cell::Cell;
mod cache;
pub(crate) use cache::Scope;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Continuation {
    Released,
    Uncertain,
    Continuing,
}

impl Continuation {
    pub(crate) fn cost(self) -> f32 {
        match self {
            Self::Released => 0.,
            Self::Uncertain => 0.05,
            Self::Continuing => 1.,
        }
    }
}

fn pulse(t: &TrackAnalysis, sec: f32) -> f32 {
    let grid = crate::grid::Grid(t);
    let mut bpm = grid.bpm(grid.beat(sec));
    if !bpm.is_finite() || bpm <= 0. {
        return 0.5;
    }
    while bpm > 160. {
        bpm *= 0.5;
    }
    while bpm < 80. {
        bpm *= 2.;
    }
    60. / bpm
}

fn present(f: &StemFrame) -> bool {
    f.rms[0].powi(2) > (f.rms[1].powi(2) + f.rms[2].powi(2)) * 0.0001
}

/// A note estimate needs an audible body. A quiet tail retains the evidence
/// from the SAME note's earlier body; a new faint estimate does not inherit
/// certainty from an unrelated loud syllable nearby. Absolute recording gain
/// cannot change this comparison.
pub(crate) fn note_supported(t: &TrackAnalysis, note: &StemNote) -> bool {
    let Some(stems) = &t.stems else { return false };
    let lo = stems
        .frames
        .partition_point(|f| f.end_sec <= note.start_sec);
    let hi = stems.frames.partition_point(|f| f.start_sec < note.end_sec);
    let frames = &stems.frames[lo..hi.max(lo)];
    if frames.iter().any(|f| present(f) && f.vocal_activity >= 0.2) {
        return true;
    }
    // Activity can miss a sustained vowel. Compare its own peak with the
    // surrounding vocal body over one perceptual bar on either side. A note
    // wholly >9 dB below that body is ambiguous, not proof of a sung sentence.
    // This grades note-only evidence; active frames still support their own note.
    let margin = 4. * pulse(t, (note.start_sec + note.end_sec) * 0.5);
    let first = stems
        .frames
        .partition_point(|f| f.end_sec <= note.start_sec - margin);
    let last = stems
        .frames
        .partition_point(|f| f.start_sec < note.end_sec + margin);
    let peak = frames
        .iter()
        .filter(|f| present(f))
        .map(|f| f.rms[0].powi(2))
        .fold(0., f32::max);
    let surrounding = stems.frames[first..last.max(first)]
        .iter()
        .map(|f| f.rms[0].powi(2))
        .fold(0., f32::max);
    peak > 0. && peak >= surrounding * 0.125
}

pub(crate) fn held_note(t: &TrackAnalysis, sec: f32) -> bool {
    t.stems.as_ref().is_some_and(|stems| {
        stems.notes[..stems.notes.partition_point(|n| n.start_sec < sec - 0.04)]
            .iter()
            .any(|n| {
                n.stem == StemKind::Vocals
                    && n.end_sec > sec + 0.04
                    && n.confidence >= 0.45
                    && note_supported(t, n)
            })
    })
}

/// Two nearby vocal observations suggest continuity, but faint remote events
/// do not prove that the same line spans their rest. Keep uncertain association
/// in ranking; active frames and supported held notes retain their protection.
pub(crate) fn continuation(t: &TrackAnalysis, sec: f32) -> Continuation {
    cache::continuation(t, sec, || measure_continuation(t, sec))
}

fn measure_continuation(t: &TrackAnalysis, sec: f32) -> Continuation {
    let Some(stems) = &t.stems else {
        return Continuation::Released;
    };
    if sec <= 0.001 || sec >= t.duration_sec - 0.001 {
        return Continuation::Released;
    }
    // Repetition connects rhythmic samples, not words in a sentence. Keep
    // each slice intact, but do not bridge its rest to the next repetition.
    if super::pattern::boundary(t, sec) {
        return Continuation::Released;
    }
    let gap = 2. * pulse(t, sec);
    let first = stems.frames.partition_point(|f| f.end_sec <= sec - gap);
    let last = stems.frames.partition_point(|f| f.start_sec < sec + gap);
    let frames = &stems.frames[first..last];
    let notes: Vec<_> = stems.notes[..stems.notes.partition_point(|n| n.start_sec < sec + gap)]
        .iter()
        .filter(|n| n.stem == StemKind::Vocals && n.confidence >= 0.45 && n.end_sec > sec - gap)
        .map(|n| (n, Cell::new(None)))
        .collect();
    // Activity is an energy estimate, not a lyric classifier. Remote, faint
    // observations cannot by themselves join two sides of a quiet interval.
    // Use the same local 9 dB body floor as note-only evidence, but never
    // downgrade activity at the boundary itself or a supported held note.
    let margin = 4. * pulse(t, sec);
    let lo = stems.frames.partition_point(|f| f.end_sec <= sec - margin);
    let hi = stems.frames.partition_point(|f| f.start_sec < sec + margin);
    let body = stems.frames[lo..hi.max(lo)]
        .iter()
        .map(|f| f.rms[0].powi(2))
        .fold(0., f32::max);
    let active_here = stems
        .frame_at(sec)
        .is_some_and(|f| present(f) && f.vocal_activity >= 0.2);
    let evidence = |f: &StemFrame| {
        if !present(f) {
            return 0;
        }
        let active = f.vocal_activity >= 0.2;
        if active && (active_here || f.rms[0].powi(2) >= body * 0.125) {
            return 2;
        }
        let at = (f.start_sec + f.end_sec) * 0.5;
        notes
            .iter()
            .filter(|(n, _)| n.start_sec < at - 0.04 && n.end_sec > at + 0.04)
            // A quiet event's own note cannot prove association with another
            // event across this rest. A note actually spanning the boundary
            // retains its supported body, including an audible held tail.
            .filter(|(n, _)| !active || (n.start_sec < sec - 0.04 && n.end_sec > sec + 0.04))
            .map(|(n, cached)| {
                let supported = cached.get().unwrap_or_else(|| {
                    let supported = note_supported(t, n);
                    cached.set(Some(supported));
                    supported
                });
                if supported {
                    2
                } else {
                    1
                }
            })
            .max()
            .unwrap_or(usize::from(active))
    };
    let mut before = [None; 2];
    let mut after = [None; 2];
    for frame in frames {
        let strength = evidence(frame);
        for threshold in 1..=strength {
            if frame.start_sec < sec - 0.05 {
                before[threshold - 1] = Some(frame.end_sec);
            }
            if frame.end_sec > sec + 0.05 && after[threshold - 1].is_none() {
                after[threshold - 1] = Some(frame.start_sec);
            }
        }
    }
    let joined = |i: usize| before[i].zip(after[i]).is_some_and(|(a, b)| b - a < gap);
    if joined(1) {
        Continuation::Continuing
    } else if joined(0) {
        Continuation::Uncertain
    } else {
        Continuation::Released
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet_observations(ratio: f32) -> TrackAnalysis {
        let mut t = crate::stem_regressions::with_stems(crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Outro,
            32,
            0.8,
            0.3,
        ));
        for f in &mut t.stems.as_mut().unwrap().frames {
            let body = (3.0..3.5).contains(&f.start_sec);
            let quiet = (4.1..4.4).contains(&f.start_sec) || (5.0..5.3).contains(&f.start_sec);
            f.vocal_activity = if body || quiet { 0.8 } else { 0. };
            f.rms = [
                if body {
                    0.1
                } else if quiet {
                    0.1 * ratio.sqrt()
                } else {
                    0.
                },
                0.1,
                0.2,
            ];
        }
        t.stems.as_mut().unwrap().notes = [(4.1, 4.4), (5.0, 5.3)]
            .into_iter()
            .map(|(start_sec, end_sec)| StemNote {
                stem: StemKind::Vocals,
                start_sec,
                end_sec,
                midi: 60,
                confidence: 0.9,
            })
            .collect();
        t
    }

    #[test]
    fn faint_remote_activity_is_uncertain_association_not_permission_to_cut_a_voice() {
        for bpm in [60., 120., 240.] {
            for speed in [0.8, 1., 1.2] {
                for gain in [0.01, 1., 4.] {
                    for shift in [0., 12.] {
                        let mut t = quiet_observations(0.04);
                        t.tempo.global_bpm = bpm / speed;
                        t.tempo.beats.clear();
                        t.tempo.segments.clear();
                        t.duration_sec = t.duration_sec * speed + shift;
                        for f in &mut t.stems.as_mut().unwrap().frames {
                            f.start_sec = f.start_sec * speed + shift;
                            f.end_sec = f.end_sec * speed + shift;
                            for rms in &mut f.rms {
                                *rms *= gain;
                            }
                        }
                        for note in &mut t.stems.as_mut().unwrap().notes {
                            note.start_sec = note.start_sec * speed + shift;
                            note.end_sec = note.end_sec * speed + shift;
                        }
                        let at = |sec: f32| sec * speed + shift;
                        assert_eq!(continuation(&t, at(4.7)), Continuation::Uncertain);
                        assert_eq!(continuation(&t, at(4.25)), Continuation::Continuing);
                        assert!(!super::super::safe_entry(&t, at(4.25)));
                        assert!(!crate::continuity::voice_cut_safe(&t, at(4.25)));
                        // A held note with an audible earlier body protects its
                        // entire tail, even through low activity and power.
                        t.stems.as_mut().unwrap().notes.push(StemNote {
                            stem: StemKind::Vocals,
                            start_sec: at(3.2),
                            end_sec: at(5.3),
                            midi: 60,
                            confidence: 0.9,
                        });
                        t.stems
                            .as_mut()
                            .unwrap()
                            .notes
                            .sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
                        assert_eq!(continuation(&t, at(4.7)), Continuation::Continuing);
                        assert!(held_note(&t, at(4.7)));
                    }
                }
            }
        }
    }

    #[test]
    fn a_consistently_quiet_phrase_keeps_its_interword_rest_protected() {
        let mut t = quiet_observations(0.04);
        for f in &mut t.stems.as_mut().unwrap().frames {
            if f.start_sec < 4. {
                f.vocal_activity = 0.;
                f.rms[0] = 0.;
            }
        }
        assert_eq!(continuation(&t, 4.7), Continuation::Continuing);
        assert!(!super::super::safe_entry(&t, 4.7));
    }

    #[test]
    fn association_strength_changes_monotonically_at_the_existing_body_floor() {
        for ratio in [0.05, 0.10, 0.12, 0.13, 0.15, 0.25] {
            let t = quiet_observations(ratio);
            assert_eq!(
                continuation(&t, 4.7),
                if ratio >= 0.125 {
                    Continuation::Continuing
                } else {
                    Continuation::Uncertain
                }
            );
            assert_eq!(continuation(&t, 4.25), Continuation::Continuing);
        }
    }

    #[test]
    fn isolated_quiet_estimates_are_uncertain_but_an_exposed_notes_tail_is_protected() {
        for bpm in [60., 120., 240.] {
            for gain in [0.01, 1., 4.] {
                for shift in [0., 12.] {
                    let mut t = crate::stem_regressions::with_stems(crate::tests::track(
                        1,
                        bpm,
                        "8A",
                        mixless_protocol::SectionLabel::Outro,
                        64,
                        0.8,
                        0.3,
                    ));
                    for f in &mut t.stems.as_mut().unwrap().frames {
                        let at = f.start_sec - shift;
                        let body = (2. ..3.).contains(&at);
                        let estimate = (3.8..4.6).contains(&at);
                        f.vocal_activity = if body { 0.9 } else { 0. };
                        f.rms = [
                            if body {
                                0.1
                            } else if estimate {
                                0.003
                            } else {
                                0.
                            },
                            0.1,
                            0.2,
                        ];
                        for v in &mut f.rms {
                            *v *= gain;
                        }
                    }
                    let note = StemNote {
                        stem: StemKind::Vocals,
                        start_sec: shift + 3.8,
                        end_sec: shift + 4.6,
                        midi: 60,
                        confidence: 0.9,
                    };
                    t.stems.as_mut().unwrap().notes = vec![note.clone()];
                    assert!(!note_supported(&t, &note));
                    assert_eq!(continuation(&t, shift + 3.6), Continuation::Uncertain);
                    assert!(!super::super::continues(&t, shift + 3.6));
                    assert!(!held_note(&t, shift + 4.2));
                    // A lingering note with a real body keeps its protection,
                    // even though the tail has the same low level as above.
                    t.stems.as_mut().unwrap().notes[0].start_sec = shift + 2.3;
                    assert_eq!(continuation(&t, shift + 3.6), Continuation::Continuing);
                    assert!(held_note(&t, shift + 4.2));
                    // A detector can miss an entire sustained foreground;
                    // strong note-local power is independent corroboration.
                    for f in &mut t.stems.as_mut().unwrap().frames {
                        f.vocal_activity = 0.;
                        if (shift + 2.3..shift + 4.6).contains(&f.start_sec) {
                            f.rms[0] = 0.1 * gain;
                        }
                    }
                    assert_eq!(continuation(&t, shift + 3.6), Continuation::Continuing);
                    assert!(held_note(&t, shift + 4.2));
                }
            }
        }
    }

    #[test]
    fn note_support_is_monotonic_near_the_relative_body_floor() {
        let mut t = crate::stem_regressions::with_stems(crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Outro,
            32,
            0.8,
            0.3,
        ));
        let note = StemNote {
            stem: StemKind::Vocals,
            start_sec: 4.,
            end_sec: 4.8,
            midi: 60,
            confidence: 0.9,
        };
        for ratio in [0.05, 0.10, 0.12, 0.13, 0.15, 0.25] {
            for f in &mut t.stems.as_mut().unwrap().frames {
                f.vocal_activity = 0.;
                f.rms[0] = if (4. ..4.8).contains(&f.start_sec) {
                    0.1 * f32::sqrt(ratio)
                } else {
                    0.1
                };
            }
            assert_eq!(note_supported(&t, &note), ratio >= 0.125);
        }
    }
}
