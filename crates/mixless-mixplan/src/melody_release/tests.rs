use super::*;
use mixless_protocol::{MixPlanSummary, Polyline, StemMix, StrategyId, TrackId};

fn ease(u: f32) -> f32 {
    let x = u.clamp(0., 1.);
    x * x * x * (10. + x * (-15. + 6. * x))
}

fn plan(trim: f32, seconds_per_bar: f32) -> MixPlan {
    let mut p = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (TrackId(1), TrackId(2)),
            strategy: StrategyId::StemSwap,
            score: 0.7,
            used_fallback: false,
            length_bars: 8,
        }),
        requires_stems: true,
        transition_mode: Some(TransitionMode::BeatBlend),
        t_in_a: 32.,
        t_out_a: 32. + 8. * seconds_per_bar,
        t_end_b: 8. * seconds_per_bar,
        outgoing_source: Polyline {
            nodes: vec![(0., 32.), (8., 32. + 8. * seconds_per_bar)],
        },
        incoming_source: Polyline {
            nodes: vec![(0., 0.), (8., 8. * seconds_per_bar)],
        },
        clock: Polyline {
            nodes: vec![(0., 0.), (8., 8. * seconds_per_bar)],
        },
        ..Default::default()
    };
    p.lanes.xfader.nodes.clear();
    p.lanes.gain_a.nodes.clear();
    p.lanes.gain_b.nodes.clear();
    let mut mix = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    for i in 0..=512 {
        let u = i as f32 / 64.;
        let g = [
            [
                1. - ease((u - 1.) / 2.),
                1. - ease(u / 2.),
                1. - ease((u - 2.) / 2.),
                1. - ease((u - 2.5) / 0.5),
            ],
            [
                ease((u - 6.) / 2.),
                ease(u / 2.),
                ease((u - 4.) / 2.),
                ease((u - 4.) / 0.5),
            ],
        ];
        let route = crate::part_routing::route(g);
        p.lanes.xfader.nodes.push((u, route.crossfader));
        p.lanes
            .gain_a
            .nodes
            .push((u, if u >= 4. { -96. } else { route.gain_db + trim }));
        p.lanes.gain_b.nodes.push((u, route.gain_db + trim));
        for s in 0..4 {
            mix.outgoing[s].nodes.push((u, route.stems[0][s]));
            mix.incoming[s].nodes.push((u, route.stems[1][s]));
        }
    }
    p.stem_mix = Some(mix);
    p
}

#[test]
fn tail_keeps_other_parts_and_inherited_entry_fixed_between_control_nodes() {
    for trim in [-24., 0., 2.] {
        for bar in [1., 2., 4.] {
            let p = plan(trim, bar);
            let q = refine(&p).unwrap();
            let mix = q.stem_mix.as_ref().unwrap();
            assert_eq!(q.lanes.xfader.nodes, p.lanes.xfader.nodes);
            assert_eq!(q.lanes.gain_b.nodes, p.lanes.gain_b.nodes);
            for s in 0..4 {
                assert_eq!(
                    mix.incoming[s].nodes,
                    p.stem_mix.as_ref().unwrap().incoming[s].nodes
                );
            }
            assert_eq!(q.clock.nodes, p.clock.nodes);
            assert_eq!(q.outgoing_source.nodes, p.outgoing_source.nodes);
            assert_eq!(q.incoming_source.nodes, p.incoming_source.nodes);
            let incoming = crate::tests::track(
                2,
                240. / bar,
                "8A",
                mixless_protocol::SectionLabel::Intro,
                32,
                0.8,
                0.5,
            );
            assert_eq!(
                crate::established_entry(&p, &incoming),
                crate::established_entry(&q, &incoming)
            );
            assert!(q.lanes.gain_a.nodes.iter().all(|(_, g)| *g <= 6.0001));
            for i in 0..=1024 {
                let u = i as f32 / 128.;
                let (a, b) = (parts(&p, u), parts(&q, u));
                for d in 0..2 {
                    for s in 0..4 {
                        if d == 0 && s == 2 {
                            assert!(b[d][s] + 0.0005 >= a[d][s]);
                            if a[d][s] == 0. {
                                assert_eq!(b[d][s], 0.);
                            }
                        } else {
                            assert!(
                                (b[d][s] - a[d][s]).abs() < 0.0005,
                                "{trim} {bar} {u} {d} {s}: {} {}",
                                a[d][s],
                                b[d][s]
                            );
                        }
                    }
                }
            }
            assert!(parts(&q, 3.)[0][2] > parts(&p, 3.)[0][2] * 1.3);
        }
    }
}

#[test]
fn never_extends_a_tail_into_another_tonal_part_or_an_intentional_reentry() {
    for part in [0, 2, 3] {
        for gain in [0.2, 0.00001] {
            let mut p = plan(0., 2.);
            p.stem_mix.as_mut().unwrap().incoming[part] = Polyline::constant(gain);
            assert!(refine(&p).is_none());
        }
    }
    let mut p = plan(0., 2.);
    p.stem_mix.as_mut().unwrap().outgoing[2] = Polyline {
        nodes: vec![(0., 1.), (1., 0.2), (2., 1.), (4., 0.)],
    };
    assert!(refine(&p).is_none());
}

#[test]
fn bass_alone_cannot_stand_in_for_a_missing_melody() {
    let mut p = plan(0., 2.);
    p.stem_mix.as_mut().unwrap().outgoing[2] = Polyline::constant(0.);
    assert!(refine(&p).is_none());
}

#[test]
fn incoming_reveal_compensates_shared_eq_without_touching_the_singer_or_kit() {
    for trim in [-24., 0., 2.] {
        let mut p = plan(trim, 2.);
        p.lanes.eq_b.mid = Polyline {
            nodes: vec![(0., 0.), (4., -9.), (6., -3.), (8., 0.)],
        };
        p.lanes.eq_b.high = Polyline {
            nodes: vec![(0., 0.), (4., -6.), (6., -2.), (8., 0.)],
        };
        let q = contour(&p, 1, 0.5, 1.).unwrap();
        assert_eq!(q.lanes.xfader.nodes, p.lanes.xfader.nodes);
        assert_eq!(q.lanes.gain_a.nodes, p.lanes.gain_a.nodes);
        assert_eq!(q.lanes.eq_b.mid.nodes, p.lanes.eq_b.mid.nodes);
        assert_eq!(q.lanes.eq_b.high.nodes, p.lanes.eq_b.high.nodes);
        let reference = parts(&p, 8.)[1][2];
        for i in 0..=1024 {
            let u = i as f32 / 128.;
            let before = parts(&p, u);
            let after = parts(&q, u);
            for d in 0..2 {
                for s in 0..4 {
                    if d == 1 && s == 2 {
                        assert!(after[d][s] + 0.0005 >= before[d][s]);
                        for eq in [&q.lanes.eq_b.mid, &q.lanes.eq_b.high] {
                            assert!(
                                after[d][s] * 10f32.powf(eq.sample(u) / 20.) <= reference + 0.0005
                            );
                        }
                        if before[d][s] == 0. {
                            assert_eq!(after[d][s], 0.);
                        }
                    } else {
                        assert!((after[d][s] - before[d][s]).abs() < 0.0005);
                    }
                }
            }
        }
        assert!(parts(&q, 5.)[1][2] > parts(&p, 5.)[1][2] * 1.3);
        assert!(q.lanes.gain_b.nodes.iter().all(|(_, g)| *g <= 6.0001));
        assert_eq!(q.t_out_a, p.t_out_a);
        assert_eq!(q.t_in_b, p.t_in_b);
        assert_eq!(q.incoming_offset_end, p.incoming_offset_end);
    }
}

#[test]
fn reveal_requires_a_sequential_tonal_exchange_and_dry_endpoints() {
    for part in [0, 2, 3] {
        let mut p = plan(0., 2.);
        // A still-sounding foreground, even a low one, is not permission to
        // raise B merely because the two tracks share a global key label.
        p.lanes.gain_a = Polyline::constant(0.);
        p.lanes.xfader = Polyline::constant(0.);
        p.stem_mix.as_mut().unwrap().outgoing[part] = Polyline::constant(0.0001);
        assert!(contour(&p, 1, 0.5, 0.).is_none());
    }
    let mut p = plan(0., 2.);
    p.lanes.eq_b.mid = Polyline::constant(-6.);
    p.lanes.eq_b.high = Polyline::constant(-6.);
    assert!(
        contour(&p, 1, 0.5, 1.).is_none(),
        "preserve the carried endpoint"
    );
}

#[test]
fn envelope_search_is_gain_invariant_and_requires_measured_bed_coverage() {
    use mixless_protocol::SectionLabel;
    let mut choices = Vec::new();
    for scale in [0.25f32, 1., 2.] {
        let mut a = crate::stem_regressions::with_stems(crate::tests::track(
            1,
            120.,
            "8A",
            SectionLabel::Outro,
            64,
            0.8,
            0.,
        ));
        a.phrase_boundaries.clear();
        for f in &mut a.stems.as_mut().unwrap().frames {
            f.vocal_activity = 0.;
            f.rms[0] = 0.;
            f.band_db[0] = [-120.; 3];
            for db in f.band_db.iter_mut().skip(1).flatten() {
                *db += 20. * scale.log10();
            }
        }
        let mut b = a.clone();
        b.track_id = TrackId(2);
        let mut p = plan(0., 2.);
        p.lanes.eq_b.mid = Polyline {
            nodes: vec![(0., 0.), (4., -9.), (8., 0.)],
        };
        p.lanes.eq_b.high = p.lanes.eq_b.mid.clone();
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        let before = cost(&p, &ctx, 1).unwrap();
        search(&mut p, &ctx, 1);
        assert!(cost(&p, &ctx, 1).unwrap() < before - 0.01);
        choices.push(p.stem_mix.as_ref().unwrap().incoming[2].nodes.clone());
        let mut unobserved = b.clone();
        unobserved.stems.as_mut().unwrap().frames.clear();
        let missing = PlanContext {
            incoming: &unobserved,
            ..ctx
        };
        assert!(cost(&p, &missing, 1).is_none());
    }
    assert_eq!(choices[0], choices[1]);
    assert_eq!(choices[1], choices[2]);
}

#[test]
fn final_voice_safety_manual_cues_and_missing_stems_remain_authoritative() {
    use mixless_protocol::{Cue, CueKind, SectionLabel};
    let mut a = crate::stem_regressions::with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        64,
        0.8,
        0.,
    ));
    a.phrase_boundaries.clear();
    for f in &mut a.stems.as_mut().unwrap().frames {
        f.vocal_activity = 0.;
        f.rms[0] = 0.;
        f.band_db[0] = [-120.; 3];
    }
    let mut b = a.clone();
    b.track_id = TrackId(2);
    let p = plan(0., 2.);
    for case in 0..7 {
        let mut a = a.clone();
        let mut q = p.clone();
        let manual = [Cue {
            index: 0,
            frame: 40 * a.sample_rate as u64,
            kind: CueKind::Out,
            user_set: true,
        }];
        if case == 2 {
            a.stems = None;
        }
        if case == 3 {
            for f in &mut a.stems.as_mut().unwrap().frames {
                f.vocal_activity = 0.9;
                f.rms[0] = 0.1;
                f.band_db[0] = [-20.; 3];
            }
        }
        if case == 4 {
            q.transition_mode = Some(TransitionMode::PhraseBridge);
        }
        if case == 5 {
            q.requires_stems = false;
        }
        if case == 6 {
            q.summary.as_mut().unwrap().length_bars = 1;
        }
        let before = serde_json::to_value(&q).unwrap();
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: if case == 1 { &manual } else { &[] },
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        arrange(&mut q, &ctx);
        assert_eq!(
            before == serde_json::to_value(&q).unwrap(),
            case != 0,
            "case {case}"
        );
    }
}
