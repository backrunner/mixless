use super::*;
use mixless_protocol::{Section, SectionLabel as L};

fn recovery_track(bpm: f32) -> TrackAnalysis {
    let mut a = crate::tests::track(1, bpm, "8A", L::Outro, 32, 0.8, 0.2);
    let bar = 240. / bpm;
    a.sections = [(0., 8., L::Intro), (8., 24., L::Drop), (24., 32., L::Outro)]
        .into_iter()
        .map(|(start, end, label)| Section {
            start_sec: start * bar,
            end_sec: end * bar,
            label,
        })
        .collect();
    a
}

fn context<'a>(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> PlanContext<'a> {
    PlanContext {
        outgoing: a,
        incoming: b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    }
}

#[test]
fn short_recovery_keeps_clock_and_carries_offset_instead_of_waiting_for_eof() {
    for scale in [0.8, 1., 1.2] {
        let a = recovery_track(128. * scale);
        let b = crate::tests::track(2, 120. * scale, "8A", L::Intro, 64, 0.8, 0.2);
        let ctx = context(&a, &b);
        let p = pair(
            &ctx,
            &PlannerOptions::default(),
            112.,
            0.,
            TransitionMode::BeatBlend,
            4,
            -1.,
        )
        .unwrap();
        assert!(p.t_out_a < a.duration_sec);
        assert_eq!(p.incoming_start_bar, 0.);
        assert!((p.incoming_offset_end.rate - 128. / 120.).abs() < 0.001);
        assert!(
            p.lanes
                .rate_a
                .nodes
                .iter()
                .all(|(_, rate)| (*rate - 1.).abs() < 0.001)
        );
        let explicit = PlannerOptions {
            strategy: Some(S::BassSwap),
            ..Default::default()
        };
        assert!(pair(&ctx, &explicit, 112., 0., TransitionMode::BeatBlend, 4, -1.).is_none());
    }
}

#[test]
fn measured_instrumental_tail_overlaps_at_native_rate_but_missing_evidence_does_not() {
    let mut a = recovery_track(128.);
    let b = crate::tests::track(2, 105., "3B", L::Intro, 64, 0.8, 0.2);
    let make = |a: &TrackAnalysis| {
        pair(
            &context(a, &b),
            &PlannerOptions::default(),
            128.,
            0.,
            TransitionMode::PhraseBridge,
            1,
            -1.,
        )
        .unwrap()
    };
    let without = make(&a);
    assert_eq!(without.summary.unwrap().length_bars, 0);
    for i in 0..(a.duration_sec / 0.05) as usize {
        a.moments.push(mixless_protocol::MusicalMoment {
            start_sec: i as f32 * 0.05,
            end_sec: (i + 1) as f32 * 0.05,
            rms: 0.2,
            band_db: [-18.; 3],
            onset: 0.,
            low_onset: 0.,
            attack_sec: 0.,
            sustain: 0.,
            chroma: [0.; 12],
            pitch_midi: None,
            vocal_confidence: Some(0.),
        });
    }
    let p = make(&a);
    assert_eq!(p.incoming_start_bar, 0.);
    assert_eq!(p.summary.unwrap().length_bars, 1);
    assert!(p.t_in_a < a.duration_sec - 1.);
    assert!(p.lanes.xfader.sample(0.5) > -0.9);
    assert_eq!(p.incoming_offset_end.rate, 1.);
    // A continuous sung ending cannot borrow the instrumental exception.
    for m in &mut a.moments {
        m.vocal_confidence = Some(1.);
    }
    assert_eq!(make(&a).summary.unwrap().length_bars, 0);
}

#[test]
fn independent_recovery_before_a_build_is_available_but_the_build_must_resolve() {
    let mut a = recovery_track(128.);
    let bar = 240. / 128.;
    a.sections.pop();
    a.sections.extend(
        [
            (24., 28., L::Break),
            (28., 30., L::BuildUp),
            (30., 32., L::Drop),
        ]
        .into_iter()
        .map(|(start, end, label)| Section {
            start_sec: start * bar,
            end_sec: end * bar,
            label,
        }),
    );
    let b = crate::tests::track(2, 128., "8A", L::Intro, 64, 0.8, 0.2);
    let ctx = context(&a, &b);
    let opts = PlannerOptions::default();
    assert!(pair(&ctx, &opts, 112., 0., TransitionMode::BeatBlend, 4, -1.).is_some());
    assert!(pair(&ctx, &opts, 120., 0., TransitionMode::BeatBlend, 4, -1.).is_none());
}
