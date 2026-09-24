use super::*;

#[test]
fn documented_score_and_selected_pair() {
    let components = score::ScoreComponents {
        phrase_align: 1.,
        section_compat: 1.,
        key_compat: 0.85,
        tempo_compat: 0.88,
        energy_continuity: 0.8,
        vocal_clash: 0.,
        kick_compat: 0.85,
        stretch_penalty: 0.,
        length_mismatch: 0.,
    };
    close(components.normalized(), 8.165 / 8.7);
    let (a, b) = pair();
    let p = plan(&a, &b);
    let s = p.summary.unwrap();
    assert_eq!(s.strategy, StrategyId::BassSwap);
    assert!(!s.used_fallback);
    close(s.score, 8.165 / 8.7);
    close(p.t_out_a, 120.);
    close(p.t_in_b, 16. * 240. / 126.);
    close(p.lanes.rate_b.sample(0.), 128. / 126.);
}
#[test]
fn all_cue_kinds_and_separate_source_rates() {
    let cue = |kind, user_set, sec| Cue {
        index: 0,
        frame: (sec * 44100.0) as u64,
        kind,
        user_set,
    };
    for kind in [CueKind::In, CueKind::Out] {
        assert!(covers_user_range(8., 12., &[cue(kind, true, 10.)], 44100));
        assert!(!covers_user_range(8., 9., &[cue(kind, true, 10.)], 44100));
        assert!(covers_user_range(8., 9., &[cue(kind, false, 10.)], 44100));
    }
    assert!(covers_user_range(
        8.,
        9.,
        &[cue(CueKind::Hot, true, 100.)],
        44100
    ));
    assert!(covers_user_range(
        5.,
        15.,
        &[cue(CueKind::Out, true, 6.), cue(CueKind::In, true, 14.)],
        44100
    ));
    let (a, mut b) = pair();
    b.sample_rate = 44100;
    let p = Planner::with_options(PlannerOptions {
        smooth: false,
        ..Default::default()
    })
    .plan_pair(
        &a,
        &b,
        &[],
        &[cue(CueKind::In, true, 1.), cue(CueKind::Out, true, 35.)],
        Default::default(),
        Default::default(),
    );
    assert!(p.summary.is_some());
    assert!(covers_user_range(
        p.t_in_b,
        p.t_end_b,
        &[cue(CueKind::In, true, 1.), cue(CueKind::Out, true, 35.)],
        44100
    ));
}
#[test]
fn impossible_cues_never_escape_through_fallback() {
    let (mut a, b) = pair();
    a.sections.clear();
    a.bars.clear();
    let cue = Cue {
        index: 0,
        frame: 999999999,
        kind: CueKind::Out,
        user_set: true,
    };
    let p = Planner::with_options(PlannerOptions {
        smooth: false,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[cue], &[], Default::default(), Default::default());
    assert!(p.summary.is_none());
    assert!(p.failure_reason.is_some());
}
#[test]
fn unknown_structure_and_short_tracks_have_constrained_fallback() {
    let a = track(1, 120., "8A", S::Unknown, 4, 0., 0.);
    let b = track(2, 120., "9A", S::Unknown, 5, 0., 0.);
    let p = plan(&a, &b);
    let s = p.summary.unwrap();
    assert!(s.used_fallback);
    assert_eq!(s.length_bars, 4);
    close(s.score, 0.);
    assert!(p.t_out_a <= a.duration_sec && p.t_end_b <= b.duration_sec);
}
#[test]
fn double_and_half_use_wall_time_not_literal_stretch() {
    for (ba, bb, faster, duration, in_bars) in
        [(160., 80., true, 24., 8.), (80., 160., false, 48., 32.)]
    {
        let a = track(1, ba, "8A", S::Drop, 64, 0.9, 0.8);
        let b = track(2, bb, "9A", S::Intro, 96, 0.8, 1.);
        let p = plan(&a, &b);
        assert_eq!(
            p.bar_map,
            BarMap::TwoToOne {
                outgoing_is_double: faster
            }
        );
        close(p.clock.sample(16.), duration);
        close(p.lanes.rate_b.sample(0.), 1.);
        close((p.t_end_b - p.t_in_b) * bb / 240., in_bars);
    }
}
#[test]
fn sounding_offsets_and_stretch_ownership() {
    let (a, b) = pair();
    for who in [WhoStretches::A, WhoStretches::B, WhoStretches::Both] {
        let p = Planner::with_options(PlannerOptions {
            smooth: false,
            who_stretches: who,
            ..Default::default()
        })
        .plan_pair(
            &a,
            &b,
            &[],
            &[],
            PerformanceOffset {
                rate: 1.04,
                pitch_semitones: 1.,
            },
            PerformanceOffset {
                rate: 0.98,
                pitch_semitones: -1.,
            },
        );
        assert!(p.summary.is_some());
        close(
            128. * p.lanes.rate_a.sample(0.),
            126. * p.lanes.rate_b.sample(0.),
        );
    }
    let same = track(3, 128., "8A", S::Intro, 64, 0.8, 1.);
    close(score::key_match(&a, &same, 1., 1.).0, 1.);
    assert!(score::key_match(&a, &same, 1., 0.).0 < 1.);
}
#[test]
fn local_tempo_map_warps_incoming_and_clock() {
    let (mut a, b) = pair();
    a.tempo.beats.clear();
    a.tempo.downbeats.clear();
    a.tempo.segments = vec![
        TempoSegment {
            start_beat: 0.,
            end_beat: 224.,
            bpm: 128.,
            confidence: 1.,
        },
        TempoSegment {
            start_beat: 224.,
            end_beat: 256.,
            bpm: 120.,
            confidence: 1.,
        },
    ];
    a.duration_sec = 121.;
    a.sections[0].end_sec = 121.;
    let p = Planner::with_options(PlannerOptions {
        smooth: false,
        strategy: Some(StrategyId::BassSwap),
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.summary.is_some());
    close(p.lanes.rate_b.sample(0.), 128. / 126.);
    close(p.lanes.rate_b.sample(12.), 120. / 126.);
    close(p.clock.sample(16.), 31.);
}
#[test]
fn energy_hold_offset_is_carried_into_next_pair() {
    let (a, _) = pair();
    let mut b = track(2, 110., "9A", S::Intro, 32, 0.8, 1.);
    b.tempo.beats.clear();
    b.tempo.downbeats.clear();
    let p = Planner::with_options(PlannerOptions {
        smooth: false,
        strategy: Some(StrategyId::EnergyHold),
        ..Default::default()
    })
    .plan_pair(
        &a,
        &b,
        &[],
        &[],
        PerformanceOffset {
            rate: 1.03,
            pitch_semitones: 1.,
        },
        Default::default(),
    );
    assert_eq!(p.summary.as_ref().unwrap().strategy, StrategyId::EnergyHold);
    close(p.incoming_offset_end.rate, 128. * 1.03 / 110.);
    assert_eq!(p.master_at(8.), mixless_protocol::DeckId::B);
    let c = track(3, 130., "9A", S::Intro, 64, 0.8, 1.);
    let next = Planner::with_options(PlannerOptions {
        smooth: false,
        ..Default::default()
    })
    .plan_pair(&b, &c, &[], &[], p.incoming_offset_end, Default::default());
    close(next.outgoing_offset.rate, p.incoming_offset_end.rate);
    close(next.lanes.rate_b.sample(0.) * 130., 128. * 1.03);
}
#[test]
fn catalog_golden_nodes_and_log_filters() {
    let (a, b) = pair();
    let planner = Planner::with_options(PlannerOptions {
        smooth: false,
        ..Default::default()
    });
    let ctx = PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    for strategy in STRATEGIES
        .into_iter()
        .chain([StrategyId::FallbackSwapFilter])
    {
        let c = planner.candidate(&ctx, 256., 0., strategy, false).unwrap();
        let p = compile::compile(&c, &a, &b, 0.8, false);
        let n = strategy.default_bars() as f32;
        close(p.lanes.xfader.sample(n), 1.);
        close(p.lanes.gain_a.sample(n), -96.);
        match strategy {
            StrategyId::BassSwap | StrategyId::FallbackSwapFilter | StrategyId::BreakToIntro => {
                close(
                    p.lanes.xfader.sample(0.),
                    if strategy == StrategyId::BreakToIntro {
                        -0.5
                    } else {
                        -0.3
                    },
                );
                close(p.lanes.xfader.sample(n / 2.), 0.2);
                close(p.lanes.eq_b.low.sample(0.), -96.);
                close(p.lanes.eq_a.low.sample(n / 2.), -96.);
            }
            StrategyId::DropCut => {
                close(p.lanes.xfader.sample(3.), -1.);
                close(p.lanes.xfader.sample(3.25), 0.);
                close(p.lanes.fx_send_a.sample(3.25), 0.8);
            }
            StrategyId::EchoOut => {
                close(p.lanes.gain_a.sample(2.), -3.);
                close(p.lanes.fx_send_a.sample(2.), 0.8);
            }
            StrategyId::LoopConstruct => {
                let op = p.lanes.loop_b.as_ref().unwrap();
                close(op.off_bar, 10.);
                assert_eq!(op.length_bars, 1);
                close(p.lanes.xfader.sample(10.), 0.4);
            }
            StrategyId::PhraseBlend | StrategyId::EnergyHold => {
                close(p.lanes.gain_b.sample(0.), -3.);
                close(p.lanes.eq_a.low.sample(n / 2.), -3.);
            }
            _ => {}
        }
        if matches!(
            strategy,
            StrategyId::FilterSweep | StrategyId::FallbackSwapFilter
        ) {
            assert!((p.lanes.filter_a.lp_hz.sample_log(n / 4.) - 4000.).abs() < 0.01);
            assert!((p.lanes.filter_b.hp_hz.sample_log(n / 4.) - 1000.).abs() < 0.01);
        }
    }
}
#[test]
fn past_playhead_and_invalid_inputs() {
    let (a, b) = pair();
    let p = Planner::with_options(PlannerOptions {
        smooth: false,
        earliest_outgoing_sec: 115.,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.summary.is_some());
    assert!(p.t_in_a >= 115.);
    let mut broken = a.clone();
    broken.duration_sec = f32::NAN;
    assert!(plan(&broken, &b).failure_reason.is_some());
    assert_eq!(PerformanceOffset::default(), PerformanceOffset::identity());
}

#[test]
fn vocal_verse_to_vocal_chorus_is_never_selected_even_as_fallback() {
    let mut a = track(1, 128., "8A", S::Verse, 64, 0.9, 1.);
    let mut b = track(2, 126., "9A", S::Chorus, 64, 0.8, 1.);
    for bar in a.bars.iter_mut().chain(b.bars.iter_mut()) {
        bar.vocal_presence = 0.9;
    }
    let p = plan(&a, &b);
    assert!(p.summary.is_none());
    assert!(p.failure_reason.is_some());
}

#[test]
fn literal_half_double_requires_opt_in_and_marks_the_plan() {
    let a = track(1, 160., "8A", S::Drop, 64, 0.9, 0.8);
    let b = track(2, 80., "9A", S::Intro, 64, 0.8, 1.);
    let p = Planner::with_options(PlannerOptions {
        smooth: false,
        literal_half_double: true,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.literal_half_double);
    assert_eq!(p.bar_map, BarMap::OneToOne);
    close(p.lanes.rate_b.sample(0.), 2.);
    assert!(p.summary.as_ref().unwrap().score < plan(&a, &b).summary.unwrap().score);
}
