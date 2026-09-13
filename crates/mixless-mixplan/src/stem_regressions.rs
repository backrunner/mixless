use super::*;
use mixless_protocol::{
    Polyline, SectionLabel, StemAnalysis, StemFrame, StemKind, StemNote, TrackId,
};

fn with_stems(mut track: TrackAnalysis) -> TrackAnalysis {
    let frames = (0..(track.duration_sec / 0.05).ceil() as usize)
        .map(|i| StemFrame {
            start_sec: i as f32 * 0.05,
            end_sec: ((i + 1) as f32 * 0.05).min(track.duration_sec),
            rms: [0.1, 0.1, 0.2],
            band_db: [[-20.; 3]; 3],
            onset: [0.; 3],
            drum_low_onset: 0.,
            vocal_activity: 0.9,
            note_chroma: std::array::from_fn(|p| if p == 0 { 1. } else { 0. }),
        })
        .collect();
    track.stems = Some(StemAnalysis {
        version: 1,
        separator_sha256: "test".into(),
        notes_sha256: "test".into(),
        duration_sec: track.duration_sec,
        residual_rms: 0.,
        frames,
        notes: vec![],
    });
    track
}

#[test]
fn stem_envelopes_follow_evidence_restore_incoming_and_leave_cuts_untouched() {
    let a = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        32,
        0.8,
        0.2,
    ));
    let mut b = a.clone();
    b.track_id = TrackId(2);
    let ctx = PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    let mut plan = MixPlan {
        summary: Some(mixless_protocol::MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: mixless_protocol::StrategyId::PhraseBlend,
            score: 1.,
            used_fallback: false,
            length_bars: 16,
        }),
        clock: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        outgoing_source: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        incoming_source: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        ..Default::default()
    };
    plan.lanes.gain_a = Polyline {
        nodes: vec![(0., 0.), (16., -36.)],
    };
    plan.lanes.gain_b = Polyline {
        nodes: vec![(0., -36.), (16., 0.)],
    };
    crate::stem_mix::arrange(&ctx, &mut plan);
    let lanes = plan.stem_mix.as_ref().unwrap();
    assert!(lanes.incoming[0].sample(4.) < 0.5);
    assert!(lanes.outgoing[0].sample(12.) < 0.5);
    assert_eq!(lanes.incoming[0].sample(16.), 1.);
    assert!(
        lanes.incoming[2].nodes.iter().all(|(_, v)| *v == 1.),
        "Consonant instruments need no suppression"
    );
    let mut cut = plan.clone();
    cut.summary.as_mut().unwrap().length_bars = 0;
    cut.stem_mix = None;
    crate::stem_mix::arrange(&ctx, &mut cut);
    assert!(cut.stem_mix.is_none());
    let mut missing = b.clone();
    missing.stems = None;
    let ctx = PlanContext {
        incoming: &missing,
        ..ctx
    };
    plan.stem_mix = None;
    crate::stem_mix::arrange(&ctx, &mut plan);
    assert!(plan.stem_mix.is_none());
}
#[test]
fn a_sung_note_cannot_be_cut_at_an_inferred_bar_or_drop_marker() {
    let mut track = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        16,
        0.7,
        0.2,
    ));
    track.stems.as_mut().unwrap().notes.push(StemNote {
        stem: StemKind::Vocals,
        start_sec: 3.,
        end_sec: 5.,
        midi: 60,
        confidence: 0.9,
    });
    assert!(!crate::continuity::cut_safe(&track, 4., true));
    assert!(!crate::vocals::safe_entry(&track, 4.));
    track.sections = vec![
        mixless_protocol::Section {
            start_sec: 0.,
            end_sec: 4.,
            label: SectionLabel::BuildUp,
        },
        mixless_protocol::Section {
            start_sec: 4.,
            end_sec: track.duration_sec,
            label: SectionLabel::Drop,
        },
    ];
    assert!(!crate::continuity::voice_cut_safe(&track, 4.));
    track.stems.as_mut().unwrap().notes[0].start_sec = 4.;
    assert!(crate::continuity::voice_cut_safe(&track, 4.));
    // A breath can release the voice, while a held instrument still disallows
    // a hard dry cut. A resolving build/drop may intentionally replace the riser.
    let stems = track.stems.as_mut().unwrap();
    stems.notes.clear();
    stems.notes.push(StemNote {
        stem: StemKind::Instruments,
        start_sec: 3.,
        end_sec: 5.,
        midi: 60,
        confidence: 0.9,
    });
    for frame in &mut stems.frames {
        frame.rms[0] = 0.;
        frame.vocal_activity = 0.;
    }
    assert!(crate::continuity::voice_cut_safe(&track, 4.));
    assert!(!crate::continuity::cut_safe(&track, 4., false));
}
#[test]
fn transcribed_progression_changes_matching_with_pitch_offset_and_rejects_corruption() {
    let a = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        16,
        0.7,
        0.2,
    ));
    let mut b = a.clone();
    b.track_id = TrackId(2);
    for frame in &mut b.stems.as_mut().unwrap().frames {
        frame.note_chroma = [0.; 12];
        frame.note_chroma[1] = 1.;
    }
    let source = Polyline {
        nodes: vec![(0., 0.), (8., 16.)],
    };
    assert!(crate::progression::tension(&a, &b, &source, &source, 8., 0., 0.) > 0.7);
    assert!(crate::progression::tension(&a, &b, &source, &source, 8., 1., 0.) < 0.01);
    b.stems.as_mut().unwrap().frames[3].rms[0] = f32::NAN;
    let plan = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(plan
        .failure_reason
        .as_deref()
        .is_some_and(|e| e.contains("stem")));
}
