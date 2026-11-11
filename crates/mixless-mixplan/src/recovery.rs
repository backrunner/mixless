//! A drop's recovery is a mixing interval, not a cue where a short fade must end.
use crate::grid::Grid;
use mixless_protocol::{SectionLabel as S, TrackAnalysis};

pub(crate) fn windows(t: &TrackAnalysis) -> Vec<(f32, f32)> {
    let mut result = mixless_protocol::relative_recoveries(t);
    for (_, drop_end) in crate::drops::peaks(t) {
        if mixless_protocol::drop_suspension(t, drop_end).is_some() {
            continue;
        }
        let Some(first) = t.sections.iter().position(|s| {
            (s.start_sec - drop_end).abs() < 0.08
                && matches!(s.label, S::Break | S::Breakdown | S::Outro)
        }) else {
            continue;
        };
        let mut end = drop_end;
        for section in &t.sections[first..] {
            if !matches!(section.label, S::Break | S::Breakdown | S::Outro) {
                break;
            }
            end = section.end_sec;
        }
        // The interval begins at the measured change in arrangement. There is
        // no mandatory bar before/after a drop and no fixed vocal-tail padding.
        let start = drop_end;
        if end > start {
            result.push((start, end));
        }
    }
    result
}

/// A relative kit valley permits part scheduling, not a whole-mix cut.
pub(crate) fn relative(t: &TrackAnalysis, sec: f32) -> bool {
    mixless_protocol::relative_recoveries(t)
        .iter()
        .any(|&(start, end)| sec >= start - 0.05 && sec <= end + 0.05)
}

pub(crate) fn contains(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    windows(t)
        .iter()
        .any(|&(a, b)| start >= a - 0.05 && end <= b + 0.05 && end > start)
}

pub(crate) fn quality(t: &TrackAnalysis, sec: f32) -> f32 {
    let g = Grid(t);
    if (g.sec(g.floor_bar(sec)) - sec).abs() < 0.08
        && windows(t)
            .iter()
            .any(|&(a, b)| sec >= a - 0.05 && sec <= b + 0.05)
    {
        0.65
    } else {
        0.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{Section, StemAnalysis, StemFrame, StemKind, StemNote};

    #[test]
    fn local_kit_retreat_opens_search_but_cannot_end_a_sung_line_or_held_note() {
        let mut t = crate::tests::track(1, 120., "8A", S::Drop, 44, 0.9, 0.5);
        t.sections[0].end_sec = 80.;
        t.sections.push(Section {
            start_sec: 80.,
            end_sec: 88.,
            label: S::Outro,
        });
        for b in &mut t.bars {
            b.vocal_presence = 1.;
            b.vocal_confidence = Some(1.);
        }
        t.stems = Some(StemAnalysis {
            version: 2,
            separator_sha256: String::new(),
            notes_sha256: String::new(),
            duration_sec: 88.,
            residual_rms: 0.,
            frames: (0..1760)
                .map(|i| {
                    let start = i as f32 * 0.05;
                    let gap = (40. ..48.).contains(&start);
                    StemFrame {
                        start_sec: start,
                        end_sec: start + 0.05,
                        rms: [0.25, if gap { 0.12 } else { 0.2 }, 0.3],
                        onset: [0., if gap { 0.12 } else { 0.2 }, 0.],
                        band_db: [[-20.; 3]; 3],
                        drum_low_onset: 0.2,
                        vocal_activity: 1.,
                        note_chroma: [0.; 12],
                    }
                })
                .collect(),
            notes: vec![
                StemNote {
                    stem: StemKind::Vocals,
                    start_sec: 38.,
                    end_sec: 43.,
                    midi: 60,
                    confidence: 0.9,
                },
                StemNote {
                    stem: StemKind::Instruments,
                    start_sec: 38.,
                    end_sec: 43.,
                    midi: 67,
                    confidence: 0.9,
                },
            ],
        });
        assert_eq!(windows(&t), [(40., 48.), (80., 88.)]);
        assert!(!crate::phrasing::exit_boundary(&t, 40.));
        assert!(relative(&t, 40.));
        assert!(crate::drops::allows(&t, 32., 40., 0., false));
        assert_eq!(
            crate::drops::penalty(&t, 80., 0.),
            0.,
            "continuing through a kit valley is not playing an extra chorus"
        );
        assert!(
            !crate::phrasing::exit_boundary(&t, 42.),
            "a low bar is not an observed phrase ending"
        );
        assert!(!crate::vocals::safe_exit(&t, 40.));
        assert!(t
            .stems
            .as_ref()
            .unwrap()
            .note_crossing(40., StemKind::Instruments));
        let source = mixless_protocol::Polyline {
            nodes: vec![(0., 32.), (8., 48.)],
        };
        assert!(
            crate::melody::windows(&t, &source, 8., (2., 4.), true).is_empty(),
            "kit withdrawal alone cannot certify a melodic phrase ending"
        );
        assert!(
            crate::drops::inside_peak(&t, 50.),
            "protect the returning peak"
        );
        assert!(
            !crate::drops::allows(&t, 58., 64., 49., false),
            "a late entry still owes its remaining peak"
        );
        // A vocal-free interval is not a missing melodic phrase ending.
        // No instrument transcription is available, so the short-handoff
        // path must not borrow the kit valley's arrangement confidence.
        let mut instrumental = t.clone();
        for b in &mut instrumental.bars {
            b.vocal_presence = 0.;
            b.vocal_confidence = Some(0.);
        }
        let stems = instrumental.stems.as_mut().unwrap();
        stems.notes.clear();
        for f in &mut stems.frames {
            f.vocal_activity = 0.;
            f.rms[0] = 0.;
        }
        let incoming = crate::tests::track(2, 120., "8A", S::Intro, 64, 0.9, 0.5);
        let ctx = crate::PlanContext {
            outgoing: &instrumental,
            incoming: &incoming,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        let handoff = crate::handoff::phrase_handoff(&ctx, &Default::default()).unwrap();
        assert!(
            handoff.t_out_a >= 80.,
            "a kit valley cannot manufacture a short whole-mix exit"
        );
        let cue = mixless_protocol::Cue {
            kind: mixless_protocol::CueKind::Out,
            frame: 60 * t.sample_rate as u64,
            index: 0,
            user_set: true,
        };
        assert_eq!(crate::phrasing::points(&t, &[cue], true, 0.), [120.]);
    }
}
