use super::*;
use mixless_protocol::{MixRegion, MixRegionKind as K, TransitionMode as M};

fn region(kind: K, start: f32, end: f32, camelot: &str) -> MixRegion {
    MixRegion {
        kind,
        start_sec: start,
        end_sec: end,
        anchor_sec: if kind == K::In { start } else { end },
        confidence: 0.9,
        vocal_risk: 0.1,
        kick: 0.9,
        rms: 0.2,
        camelot: Some(camelot.into()),
        key_confidence: 0.95,
    }
}

#[test]
fn harmonic_correction_is_prepared_and_held_without_an_audible_pitch_ramp() {
    let a = track(1, 128., "8A", S::Outro, 64, 0.9, 0.2);
    let b = track(2, 126., "3A", S::Intro, 64, 0.9, 0.2);
    let opts = PlannerOptions {
        harmonic_key_shift: true,
        ..Default::default()
    };
    let p = Planner::with_options(opts).plan_pair(
        &a,
        &b,
        &[],
        &[],
        Default::default(),
        Default::default(),
    );
    assert_eq!(p.transition_mode, Some(M::BeatBlend));
    assert_eq!(p.lanes.pitch_b.nodes, vec![(0., -1.)]);
    assert_eq!(p.incoming_offset_end.pitch_semitones, -1.);
    assert!(p.lanes.pitch_a.nodes.iter().all(|(_, p)| *p == 0.));
    let legacy = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_ne!(legacy.transition_mode, Some(M::BeatBlend));
}

#[test]
fn local_window_key_overrides_the_global_key_and_low_confidence_cannot_authorize_a_shift() {
    let mut a = track(1, 120., "1A", S::Outro, 64, 0.9, 0.2);
    let mut b = track(2, 120., "1A", S::Intro, 64, 0.9, 0.2);
    a.mix_regions = vec![region(K::Out, 96., 128., "8A")];
    b.mix_regions = vec![region(K::In, 0., 32., "3A"), region(K::In, 32., 64., "8A")];
    assert_eq!(
        score::window_key_match(&a, &b, 128., 0., 0., 0.),
        (0.5, -1., true)
    );
    assert_eq!(
        score::window_key_match(&a, &b, 128., 32., 0., 0.),
        (1., 0., true)
    );
    b.mix_regions[0].key_confidence = 0.1;
    assert!(!score::window_key_match(&a, &b, 128., 0., 0., 0.).2);
}

#[test]
fn planning_compares_several_windows_and_keeps_the_overlap_inside_them() {
    let mut a = track(1, 120., "8A", S::Outro, 64, 0.9, 0.2);
    let mut b = track(2, 120., "8A", S::Intro, 64, 0.9, 0.2);
    a.mix_regions = vec![
        region(K::Out, 80., 96., "8A"),
        region(K::Out, 112., 128., "8A"),
    ];
    b.mix_regions = vec![region(K::In, 0., 16., "8A"), region(K::In, 32., 48., "8A")];
    // The first entry is a vocal foreground; the later entry is a clear intro.
    for bar in &mut a.bars {
        bar.vocal_presence = 0.8;
    }
    for bar in &mut b.bars {
        if bar.start_sec < 32. {
            bar.vocal_presence = 0.8;
        }
    }
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(p.transition_mode, Some(M::BeatBlend));
    assert_eq!(p.t_in_b, 32.);
    assert!(p.t_end_b <= 48.01);
    // Region 112..128 only admits eight bars, not a longer blend through 96..112.
    if (p.t_out_a - 128.).abs() < 0.01 {
        assert!(p.t_in_a >= 111.99);
    }
}

#[test]
fn an_auto_pad_adds_a_manual_entry_between_analyzed_phrase_candidates() {
    let a = track(1, 120., "8A", S::Outro, 64, 0.9, 0.2);
    let mut b = track(2, 120., "8A", S::Intro, 64, 0.9, 0.2);
    b.phrase_boundaries = [0., 36.]
        .into_iter()
        .map(|time_sec| mixless_protocol::PhraseBoundary {
            time_sec,
            confidence: 0.9,
            novelty: 0.5,
        })
        .collect();
    for bar in &mut b.bars {
        if bar.start_sec < 20. {
            bar.rms = 0.;
        }
    }
    let cue = Cue {
        index: 3,
        frame: 20 * 48000,
        kind: CueKind::Hot,
        user_set: true,
    };
    assert_eq!(
        crate::cue_policy::inferred_cue_kind(&b, cue.frame),
        CueKind::In
    );
    let points = crate::phrasing::points(&b, &[cue.clone()], false, 0.);
    assert!(points.contains(&40.));
    let plan =
        Planner::new().plan_pair(&a, &b, &[], &[cue], Default::default(), Default::default());
    assert!(plan.failure_reason.is_none(), "{:?}", plan.failure_reason);
    assert_eq!(plan.t_in_b, 20.);
}

#[test]
fn late_native_bridge_respects_manual_cues_and_rejects_out_of_audio_markers() {
    let a = track(1, 120., "8A", S::Outro, 4, 0.9, 0.2);
    let b = track(2, 120., "8A", S::Intro, 4, 0.9, 0.2);
    let mut cues = vec![Cue {
        index: 0,
        frame: 5 * 48000,
        kind: CueKind::In,
        user_set: true,
    }];
    let plan = |cues: &[Cue]| {
        short_handoff(
            &PlanContext {
                outgoing: &a,
                incoming: &b,
                cues_out: &[],
                cues_in: cues,
                offset_a: Default::default(),
                offset_b: Default::default(),
            },
            6.5,
        )
    };
    let p = plan(&cues);
    assert!(p.failure_reason.is_none());
    assert_eq!(p.t_in_b, 5.);
    assert!(p.t_in_a >= 6.5 && p.t_out_a <= 8. && p.t_end_b <= 8.);
    cues[0].frame = 9 * 48000;
    assert!(plan(&cues).failure_reason.is_some());
}
