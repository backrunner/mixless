use super::*;
use mixless_protocol::{MixPlanSummary, SectionLabel, StemKind, StemNote, StrategyId, TrackId};

fn fixture(
    scale: f32,
) -> (
    mixless_protocol::TrackAnalysis,
    mixless_protocol::TrackAnalysis,
    MixPlan,
) {
    let mut a = crate::stem_regressions::with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        64,
        0.9,
        0.2,
    ));
    let mut b = a.clone();
    b.track_id = TrackId(2);
    for (d, t) in [&mut a, &mut b].into_iter().enumerate() {
        for f in &mut t.stems.as_mut().unwrap().frames {
            f.vocal_activity = if d == 0 && f.start_sec < 22. { 0.9 } else { 0. };
            f.band_db = [[-120.; 3], [-25., -32., -25.], [-24., -22., -30.]];
            if f.vocal_activity > 0. {
                f.band_db[0] = [-40., -24., -36.];
            }
            for v in f.band_db.iter_mut().flatten() {
                *v += 20. * scale.log10();
            }
        }
    }
    let p = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: StrategyId::BassSwap,
            score: 0.8,
            length_bars: 16,
            used_fallback: false,
        }),
        t_in_a: 0.,
        t_out_a: 32.,
        t_in_b: 0.,
        t_end_b: 32.,
        outgoing_source: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        incoming_source: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        clock: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        handoff_bar: Some(6.),
        ..Default::default()
    };
    (a, b, p)
}
fn ctx<'a>(
    a: &'a mixless_protocol::TrackAnalysis,
    b: &'a mixless_protocol::TrackAnalysis,
) -> PlanContext<'a> {
    PlanContext {
        outgoing: a,
        incoming: b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    }
}
fn heard(p: &MixPlan, d: usize, s: usize, u: f32) -> f32 {
    let x = (p.lanes.xfader.sample(u) + 1.) / 2.;
    let mix = p.stem_mix.as_ref().unwrap();
    let (fade, gain, part) = if d == 0 {
        (
            (1. - x).sqrt(),
            p.lanes.gain_a.sample(u),
            mix.outgoing[s].sample(u),
        )
    } else {
        (
            x.sqrt(),
            p.lanes.gain_b.sample(u),
            mix.incoming[s].sample(u),
        )
    };
    fade * 10f32.powf(gain / 20.) * part
}

#[test]
fn vocal_completes_over_the_new_bed_with_independent_levels_and_gain_invariant_timing() {
    let mut reference = None;
    for scale in [0.25, 1., 2.] {
        let (a, b, mut p) = fixture(scale);
        arrange(&mut p, &ctx(&a, &b), true).unwrap();
        for i in 1..22 * 8 {
            let u = i as f32 / 16.;
            assert!(heard(&p, 0, 0, u) > 0.99, "voice faded at {u}");
        }
        assert_eq!(p.lanes.xfader.sample(8.), 0.);
        assert!(
            heard(&p, 1, 1, 10.) > 0.9,
            "new bed underneath the still-singing voice"
        );
        assert!(heard(&p, 0, 0, 14.) < 0.001);
        assert!(
            p.lanes.eq_a.mid.sample(10.) > -0.01,
            "sung foreground stays clear"
        );
        for s in 0..4 {
            assert!((heard(&p, 1, s, 16.) - 1.).abs() < 1e-5);
        }
        let nodes = p.stem_mix.as_ref().unwrap().outgoing[0].nodes.clone();
        if let Some(ref previous) = reference {
            assert_eq!(&nodes, previous);
        } else {
            reference = Some(nodes);
        }
    }
}

#[test]
fn sustained_words_missing_coverage_and_false_gaps_cannot_authorize_a_fade() {
    let (mut a, b, p) = fixture(1.);
    for f in &mut a.stems.as_mut().unwrap().frames {
        f.vocal_activity = 0.9;
    }
    assert!(arrange(&mut p.clone(), &ctx(&a, &b), true).is_none());
    // One low frame between syllables cannot be a breath.
    a.stems.as_mut().unwrap().frames[440].vocal_activity = 0.;
    assert!(evidence::voice_windows(&p.outgoing_source, &a, 16., 0.125).is_empty());
    for f in &mut a.stems.as_mut().unwrap().frames {
        f.vocal_activity = 0.;
    }
    a.stems.as_mut().unwrap().notes.push(StemNote {
        stem: StemKind::Vocals,
        start_sec: 0.,
        end_sec: 32.,
        midi: 60,
        confidence: 0.9,
    });
    assert!(evidence::voice_windows(&p.outgoing_source, &a, 16., 0.125).is_empty());
    a.stems.as_mut().unwrap().notes.clear();
    a.stems.as_mut().unwrap().frames.clear();
    assert!(arrange(&mut p.clone(), &ctx(&a, &b), true).is_none());
}

#[test]
fn incompatible_keys_never_share_tonal_parts() {
    let (a, b, mut p) = fixture(1.);
    arrange(&mut p, &ctx(&a, &b), false).unwrap();
    for i in 0..=1024 {
        let u = i as f32 / 64.;
        let tonal = |d| {
            [0, 2, 3]
                .into_iter()
                .map(|s| heard(&p, d, s, u))
                .fold(0f32, f32::max)
        };
        assert!(tonal(0) * tonal(1) < 0.0002, "keys overlap at {u}");
    }
}

#[test]
fn fast_syllables_do_not_allow_fading_a_continuous_sung_line() {
    let (mut a, b, mut p) = fixture(1.);
    for f in &mut a.stems.as_mut().unwrap().frames {
        f.vocal_activity = 1.;
    }
    a.stems.as_mut().unwrap().notes = (0..128)
        .map(|i| StemNote {
            stem: StemKind::Vocals,
            start_sec: i as f32 * 0.25,
            end_sec: i as f32 * 0.25 + 0.2,
            midi: 60,
            confidence: 0.9,
        })
        .collect();
    assert!(
        crate::vocals::chops(&a, 0., 32.),
        "the old short-note heuristic admits this line"
    );
    p.lanes.xfader = Polyline {
        nodes: vec![(0., -1.), (16., 1.)],
    };
    p.lanes.gain_a = Polyline::constant(0.);
    p.lanes.gain_b = Polyline::constant(0.);
    for filter in [&mut p.lanes.filter_a, &mut p.lanes.filter_b] {
        filter.lp_hz = Polyline::constant(20000.);
        filter.hp_hz = Polyline::constant(20.);
    }
    assert!(!crate::foreground::preserved(&p, &ctx(&a, &b)));
}

#[test]
fn a_short_rest_near_the_end_has_the_same_local_clock_as_an_earlier_rest() {
    let (mut a, _, p) = fixture(1.);
    for f in &mut a.stems.as_mut().unwrap().frames {
        f.vocal_activity = 1.;
    }
    for f in &mut a.stems.as_mut().unwrap().frames {
        if f.start_sec >= 31.6 {
            f.vocal_activity = 0.;
        }
    }
    let windows = evidence::voice_windows(&p.outgoing_source, &a, 16., 0.125);
    assert!(windows.iter().any(|f| f.start >= 15.8 && f.end <= 16.));
}

#[test]
fn an_arrangement_cannot_remove_a_sung_foreground_at_its_first_sample() {
    let (a, b, mut p) = fixture(1.);
    arrange(&mut p, &ctx(&a, &b), true).unwrap();
    assert!(crate::foreground::preserved(&p, &ctx(&a, &b)));
    p.stem_mix.as_mut().unwrap().outgoing[0] = Polyline::constant(0.);
    assert!(!crate::foreground::preserved(&p, &ctx(&a, &b)));
}

#[test]
fn a_quiet_held_tail_cannot_be_faded_after_the_main_voice_activity_drops() {
    let (mut a, b, mut p) = fixture(1.);
    arrange(&mut p, &ctx(&a, &b), true).unwrap();
    for f in &mut a.stems.as_mut().unwrap().frames {
        if f.start_sec >= 22. {
            f.vocal_activity = 0.1;
        }
    }
    a.stems.as_mut().unwrap().notes.push(StemNote {
        stem: StemKind::Vocals,
        start_sec: 21.9,
        end_sec: 23.,
        midi: 60,
        confidence: 0.9,
    });
    assert!(!crate::foreground::preserved(&p, &ctx(&a, &b)));
    let mut safe = p.clone();
    arrange(&mut safe, &ctx(&a, &b), true).unwrap();
    assert!(crate::foreground::preserved(&safe, &ctx(&a, &b)));
}

#[test]
fn a_loud_chord_reveal_gets_time_instead_of_an_instant_power_handoff() {
    let (a, b, mut p) = fixture(1.);
    arrange(&mut p, &ctx(&a, &b), true).unwrap();
    let mut maximum = 0f32;
    for i in 1..1024 {
        let u = 16. * i as f32 / 1024.;
        let previous = u - 16. / 1024.;
        let dt = p.clock.sample(u) - p.clock.sample(previous);
        maximum = maximum.max((heard(&p, 1, 2, u) - heard(&p, 1, 2, previous)).abs() / dt);
    }
    assert!(maximum < 0.5, "instrument gain changed at {maximum}/s");
}

#[test]
fn synchronized_stems_describe_the_actual_track_fade_or_cut() {
    for scale in [0.25, 1., 2.] {
        let (a, b, mut p) = fixture(scale);
        let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
        let mut shape = [[Fade { start: 6., end: 8. }; 4]; 2];
        render::apply(&mut p, &frames, &shape).unwrap();
        assert_eq!(
            p.summary.as_ref().unwrap().strategy,
            StrategyId::PhraseBlend
        );
        assert_eq!(p.stages[0].label, "Wait for incoming entry");
        assert_eq!(p.stages[0].end_bar, 6.);
        assert_eq!(p.stages[1].label, "Crossfade tracks");
        for s in 0..4 {
            assert!((heard(&p, 0, s, 7.) - 0.5).abs() < 1e-5);
            assert!((heard(&p, 1, s, 7.) - 0.5).abs() < 1e-5);
        }
        shape[1] = [Fade { start: 8., end: 9. }; 4];
        render::apply(&mut p, &frames, &shape).unwrap();
        assert_eq!(
            p.summary.as_ref().unwrap().strategy,
            StrategyId::PhraseBlend
        );
        assert_eq!(p.stages[1].label, "Fade between tracks");
        shape = [[Fade { start: 8., end: 8. }; 4]; 2];
        render::apply(&mut p, &frames, &shape).unwrap();
        assert_eq!(p.summary.as_ref().unwrap().strategy, StrategyId::DryCut);
        assert_eq!(p.stages[1].label, "Switch tracks");
    }
}

#[test]
fn a_silent_lead_in_does_not_disqualify_a_later_real_stem_exchange() {
    let (a, b, mut p) = fixture(1.);
    let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
    let mut shape = [[Fade {
        start: 8.,
        end: 10.,
    }; 4]; 2];
    shape[0][1] = Fade { start: 6., end: 7. };
    shape[1][1] = shape[0][1];
    render::apply(&mut p, &frames, &shape).unwrap();
    assert_eq!(p.summary.as_ref().unwrap().strategy, StrategyId::StemSwap);
    for s in 0..4 {
        assert_eq!(heard(&p, 1, s, 4.), 0.);
    }
    assert_eq!(
        p.stage_at_elapsed(8.).unwrap().label,
        "Wait for incoming entry"
    );
    assert!(heard(&p, 1, 1, 7.5) > 0.99);
    assert!(heard(&p, 0, 2, 7.5) > 0.99);
    assert_eq!(heard(&p, 1, 2, 7.5), 0.);
}

#[test]
fn moving_an_absent_vocal_does_not_turn_a_track_blend_into_a_stem_swap() {
    let (mut a, mut b, mut p) = fixture(1.);
    for t in [&mut a, &mut b] {
        for f in &mut t.stems.as_mut().unwrap().frames {
            f.vocal_activity = 0.;
            f.band_db[0] = [-120.; 3];
        }
    }
    let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
    let mut shape = [[Fade { start: 6., end: 8. }; 4]; 2];
    shape[0][0] = Fade {
        start: 11.,
        end: 11.25,
    };
    shape[1][0] = Fade {
        start: 11.25,
        end: 11.5,
    };
    render::apply(&mut p, &frames, &shape).unwrap();
    assert_eq!(
        p.summary.as_ref().unwrap().strategy,
        StrategyId::PhraseBlend
    );
}
