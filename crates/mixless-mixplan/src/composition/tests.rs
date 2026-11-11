use super::*;

#[test]
fn distinct_rests_survive_dense_placements_in_one_breath() {
    let mut windows: Vec<_> = (0..100)
        .map(|i| Fade {
            start: i as f32 * 0.01,
            end: i as f32 * 0.01 + 0.2,
        })
        .collect();
    windows.extend([
        Fade {
            start: 4.,
            end: 4.25,
        },
        Fade {
            start: 8.,
            end: 8.25,
        },
    ]);
    let choices = alternatives::rests(&windows, 0.5);
    assert_eq!(choices.len(), 3);
    assert!(choices.iter().any(|f| f.start == 4.));
    assert!(choices.iter().any(|f| f.start == 8.));
}

#[test]
fn continuing_kit_can_remain_after_a_complete_foreground_exchange() {
    for scale in [0.01, 1., 10.] {
        let (mut a, mut b, mut p) = fixture(scale);
        a.sections = vec![
            mixless_protocol::Section {
                start_sec: 0.,
                end_sec: 32.,
                label: SectionLabel::Drop,
            },
            mixless_protocol::Section {
                start_sec: 32.,
                end_sec: 128.,
                label: SectionLabel::Outro,
            },
        ];
        for (t, at) in [(&mut a, 16.), (&mut b, 12.)] {
            t.phrase_boundaries = vec![mixless_protocol::PhraseBoundary {
                time_sec: at,
                confidence: 0.9,
                novelty: 0.9,
            }];
        }
        for (d, t) in [&mut a, &mut b].into_iter().enumerate() {
            for f in &mut t.stems.as_mut().unwrap().frames {
                let sung = d == 0 && (f.start_sec < 8. || (16. ..24.).contains(&f.start_sec));
                f.vocal_activity = if sung { 0.9 } else { 0. };
                f.band_db[0] = if sung { [-30., -22., -30.] } else { [-120.; 3] };
                for value in &mut f.band_db[0] {
                    *value += 20. * scale.log10();
                }
                if d == 1 {
                    for value in &mut f.band_db[2] {
                        *value += 6.;
                    }
                }
                f.rms = std::array::from_fn(|s| {
                    f.band_db[s]
                        .iter()
                        .map(|v| 10f32.powf(*v / 10.))
                        .sum::<f32>()
                        .sqrt()
                });
            }
        }
        let mut shape = [[Fade {
            start: 15.,
            end: 16.,
        }; 4]; 2];
        shape[0][0] = Fade {
            start: 4.25,
            end: 4.5,
        };
        shape[0][2] = Fade { start: 6., end: 8. };
        shape[1][2] = Fade { start: 6., end: 8. };
        shape[0][3] = Fade {
            start: 7.75,
            end: 8.,
        };
        shape[1][3] = Fade {
            start: 8.,
            end: 8.25,
        };
        let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert!(
            crate::foreground::preserved(&p, &ctx(&a, &b)),
            "a complete old line can leave while the new melody owns the foreground"
        );
        shape[0][0] = Fade {
            start: 3.5,
            end: 4.,
        };
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert!(
            !crate::foreground::preserved(&p, &ctx(&a, &b)),
            "new foreground never authorizes truncating an active word"
        );
    }
}

#[test]
fn incoming_peak_uses_audible_weight_instead_of_a_silent_lane_deadline() {
    for (scale, speed, offset) in [(0.01, 0.8, 9.), (1., 1., 0.), (10., 1.45, 37.)] {
        let (mut a, mut b, mut p) = fixture(scale);
        b.sections = vec![
            mixless_protocol::Section {
                start_sec: 0.,
                end_sec: 8.,
                label: SectionLabel::Intro,
            },
            mixless_protocol::Section {
                start_sec: 8.,
                end_sec: 128.,
                label: SectionLabel::Drop,
            },
        ];
        // Transform source evidence and mapping together. This tests planner
        // coordinates; it is not a claim about re-extracted audio features.
        let time = |sec: f32| sec / speed + offset;
        for t in [&mut a, &mut b] {
            t.duration_sec = time(t.duration_sec);
            t.tempo.global_bpm *= speed;
            for segment in &mut t.tempo.segments {
                segment.bpm *= speed;
            }
            for sec in t.tempo.beats.iter_mut().chain(&mut t.tempo.downbeats) {
                *sec = time(*sec);
            }
            for section in &mut t.sections {
                section.start_sec = time(section.start_sec);
                section.end_sec = time(section.end_sec);
            }
            for bar in &mut t.bars {
                bar.start_sec = time(bar.start_sec);
                bar.end_sec = time(bar.end_sec);
            }
            for phrase in &mut t.phrase_boundaries {
                phrase.time_sec = time(phrase.time_sec);
            }
            let stems = t.stems.as_mut().unwrap();
            stems.duration_sec = t.duration_sec;
            for frame in &mut stems.frames {
                frame.start_sec = time(frame.start_sec);
                frame.end_sec = time(frame.end_sec);
            }
            for note in &mut stems.notes {
                note.start_sec = time(note.start_sec);
                note.end_sec = time(note.end_sec);
            }
            if offset > 0. {
                let mut padding = stems.frames[0].clone();
                padding.start_sec = 0.;
                padding.end_sec = offset;
                stems.frames.insert(0, padding);
            }
        }
        p.t_in_a = time(p.t_in_a);
        p.t_out_a = time(p.t_out_a);
        p.t_in_b = time(p.t_in_b);
        p.t_end_b = time(p.t_end_b);
        for (_, sec) in p
            .outgoing_source
            .nodes
            .iter_mut()
            .chain(&mut p.incoming_source.nodes)
        {
            *sec = time(*sec);
        }
        for (_, sec) in &mut p.clock.nodes {
            *sec /= speed;
        }
        let mut shape = [[Fade { start: 3., end: 4. }; 4]; 2];
        shape[1][0] = Fade { start: 8., end: 9. };
        let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
        let reference = [
            crate::level::reference_power(&a).unwrap(),
            crate::level::reference_power(&b).unwrap(),
        ];
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert!(arrival::preserved(&p, &ctx(&a, &b), reference));
        // Keeping only the kit cannot stand in for a removed melodic arrival.
        for part in [0, 2, 3] {
            shape[1][part] = Fade {
                start: 12.,
                end: 13.,
            };
        }
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert!(!arrival::preserved(&p, &ctx(&a, &b), reference));
    }
}

#[test]
fn a_complementary_exchange_can_cross_a_brief_power_trough() {
    for scale in [0.01, 1., 10.] {
        let fs: Vec<_> = (0..=128)
            .map(|i| {
                let mut bands = [[[0.; 3]; 4]; 2];
                for (d, deck) in bands.iter_mut().enumerate() {
                    let weight = if d == 0 { 1. } else { 0.7 } * scale;
                    deck[1] = [weight * 0.2; 3];
                    deck[2] = [0., weight * 0.2, weight * 0.2];
                }
                Frame {
                    u: i as f32 / 16.,
                    sec: i as f32 / 8.,
                    bands,
                    voice: [0.; 2],
                }
            })
            .collect();
        let shape = [[Fade {
            start: 3.5,
            end: 4.5,
        }; 4]; 2];
        // A linear complementary amplitude swap is below half of A's power
        // around its crossing when B is slightly quieter. The complete bar,
        // after routing, remains the authority on sustained peak weight.
        let midpoint = power(&fs[64], gains(&shape, 4.));
        assert!(midpoint.iter().flatten().sum::<f32>() < scale * 0.5);
        assert!(cost(&fs, &shape, Tonality::Compatible).is_some());
    }
}

#[test]
fn a_long_removed_bed_needs_a_carried_foreground_not_just_loud_drums() {
    for scale in [0.01, 1., 10.] {
        let mut fs: Vec<_> = (0..=32)
            .map(|i| {
                let mut bands = [[[0.; 3]; 4]; 2];
                for d in &mut bands {
                    d[1] = [0.1 * scale; 3];
                    d[2] = [0., 0.2 * scale, 0.2 * scale];
                }
                evidence::Frame {
                    u: i as f32 / 4.,
                    sec: i as f32 / 2.,
                    bands,
                    voice: [0.; 2],
                }
            })
            .collect();
        let mut shape = [[Fade { start: 7., end: 8. }; 4]; 2];
        shape[1][1] = Fade {
            start: 0.,
            end: 0.5,
        };
        shape[0][2] = Fade { start: 0., end: 1. };
        shape[1][2] = Fade { start: 5., end: 6. };
        assert!(cost(&fs, &shape, Tonality::Compatible).is_none());
        // A singer carrying the phrase across the exchange is a real foreground.
        for f in &mut fs {
            f.bands[0][0] = [0.1 * scale; 3];
            f.voice[0] = 1.;
        }
        assert!(cost(&fs, &shape, Tonality::Compatible).is_some());
        // Natural silence is not a control-induced hole either.
        for f in &mut fs {
            f.bands[0][0] = [0.; 3];
            f.voice[0] = 0.;
            if f.u >= 1. && f.u <= 5. {
                for d in &mut f.bands {
                    d[2] = [0.; 3];
                }
            }
        }
        assert!(cost(&fs, &shape, Tonality::Compatible).is_some());
    }
}
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

#[test]
fn note_candidates_retain_previously_viable_structural_arrangements() {
    let (mut a, b, p) = fixture(1.);
    a.phrase_boundaries = vec![mixless_protocol::PhraseBoundary {
        time_sec: 20.,
        confidence: 0.9,
        novelty: 0.5,
    }];
    let shape = [[Fade {
        start: 8.,
        end: 12.,
    }; 4]; 2];
    let before = phrasing::shapes(
        &mut crate::melody::Search::new(&ctx(&a, &b), &p, 16.),
        &p,
        &ctx(&a, &b),
        shape,
        &[],
        Tonality::Compatible,
        0.,
        16.,
    );
    assert!(!before.is_empty());
    let stems = a.stems.as_mut().unwrap();
    stems.notes = [
        (12., 13., 60),
        (13.1, 14., 64),
        (14.1, 16., 67),
        (18., 20., 65),
    ]
    .into_iter()
    .map(|(start_sec, end_sec, midi)| StemNote {
        stem: StemKind::Instruments,
        start_sec,
        end_sec,
        midi,
        confidence: 0.9,
    })
    .collect();
    for f in &mut stems.frames {
        if f.start_sec >= 16. && f.end_sec <= 18. {
            f.rms[2] *= 0.1;
        }
    }
    assert!(!mixless_protocol::instrument_boundaries(&a).is_empty());
    let after = phrasing::shapes(
        &mut crate::melody::Search::new(&ctx(&a, &b), &p, 16.),
        &p,
        &ctx(&a, &b),
        shape,
        &[],
        Tonality::Compatible,
        0.,
        16.,
    );
    for old in &before {
        assert!(after.iter().any(|new| {
            old.iter()
                .flatten()
                .zip(new.iter().flatten())
                .all(|(a, b)| a.start == b.start && a.end == b.end)
        }));
    }
    assert!(after.len() > before.len());
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
        arrange(&mut p, &ctx(&a, &b), Tonality::Compatible, duration_reward).unwrap();
        let heard_bars = crate::experience::overlap_bars(&p, &a, &b);
        assert!(
            0.8 - p.summary.as_ref().unwrap().score + 1e-5
                >= duration_reward(16.) - duration_reward(heard_bars),
            "the fitted score must pay for time made inaudible by final routing"
        );
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
fn optimistic_participation_never_prunes_a_better_rendered_overlap() {
    for scale in [0.25, 1., 4.] {
        let (a, b, mut p) = fixture(scale);
        p.incoming_start_bar = 0.5;
        let ctx = ctx(&a, &b);
        let frames = frames(&p, &ctx).unwrap();
        let samples = participation::samples(&p, &ctx, 16.).unwrap();
        let reference = [
            crate::level::reference_power(&a).unwrap(),
            crate::level::reference_power(&b).unwrap(),
        ];
        for at in [0.63, 4.13, 8.13, 12.13] {
            for width in [0.125, 0.5, 2.] {
                let mut shape = [[Fade {
                    start: at,
                    end: at + width,
                }; 4]; 2];
                shape[1][1] = Fade {
                    start: 0.5,
                    end: 1.,
                };
                let mut rendered = p.clone();
                render::apply(&mut rendered, &frames, &shape, &ctx).unwrap();
                assert!(
                    participation::upper(&samples, &shape, reference)
                        >= crate::experience::overlap_bars(&rendered, &a, &b)
                );
            }
        }
    }
}

#[test]
fn sustained_words_missing_coverage_and_false_gaps_cannot_authorize_a_fade() {
    let (mut a, b, p) = fixture(1.);
    for f in &mut a.stems.as_mut().unwrap().frames {
        f.vocal_activity = 0.9;
    }
    assert!(arrange(
        &mut p.clone(),
        &ctx(&a, &b),
        Tonality::Compatible,
        duration_reward
    )
    .is_none());
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
    assert!(arrange(
        &mut p.clone(),
        &ctx(&a, &b),
        Tonality::Compatible,
        duration_reward
    )
    .is_none());
}

#[test]
fn incompatible_keys_share_tonal_parts_for_at_most_one_exchange_bar() {
    let (a, b, mut p) = fixture(1.);
    arrange(&mut p, &ctx(&a, &b), Tonality::Conflict, duration_reward).unwrap();
    let mut shared = 0.;
    for i in 0..=1024 {
        let u = i as f32 / 64.;
        let tonal = |d| {
            [0, 2, 3]
                .into_iter()
                .map(|s| heard(&p, d, s, u))
                .fold(0f32, f32::max)
        };
        // Counted above -40 dB on both decks, as the fitted cost bounds it
        // at its 1/128 evidence resolution.
        if tonal(0) > 0.01 && tonal(1) > 0.01 {
            shared += 1. / 64.;
        }
        assert!(
            heard(&p, 0, 0, u) * heard(&p, 1, 0, u) < 0.0002,
            "voices overlap at {u}"
        );
    }
    assert!(
        shared <= 1. + p.summary.as_ref().unwrap().length_bars as f32 / 128.,
        "keys share {shared} bars"
    );
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
        !crate::vocals::chops(&a, 0., 32.),
        "short notes without repeated vocal envelopes are not chop evidence"
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
fn repeated_vocal_samples_offer_fade_windows_without_inventing_a_sung_rest() {
    let a = crate::vocals::chop_tests::track(true);
    let source = Polyline {
        nodes: vec![(0., 4.), (4., 12.)],
    };
    let windows = super::evidence::voice_windows(&source, &a, 4., 0.5);
    assert!(windows.iter().any(|f| (f.start - 1.).abs() < 0.01));
    let mut sung = a;
    for f in &mut sung.stems.as_mut().unwrap().frames {
        f.vocal_activity = 1.;
        f.rms[0] = 0.05;
    }
    assert!(super::evidence::voice_windows(&source, &sung, 4., 0.5).is_empty());
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
    arrange(&mut p, &ctx(&a, &b), Tonality::Compatible, duration_reward).unwrap();
    assert!(crate::foreground::preserved(&p, &ctx(&a, &b)));
    p.stem_mix.as_mut().unwrap().outgoing[0] = Polyline::constant(0.);
    assert!(!crate::foreground::preserved(&p, &ctx(&a, &b)));
}

#[test]
fn a_safe_source_entry_cannot_reveal_the_incoming_singer_halfway_through_a_word() {
    let (a, mut b, mut p) = fixture(1.);
    arrange(&mut p, &ctx(&a, &b), Tonality::Compatible, duration_reward).unwrap();
    for f in &mut b.stems.as_mut().unwrap().frames {
        f.vocal_activity = 0.9;
        f.rms[0] = f.rms[1].max(f.rms[2]).max(0.1);
    }
    b.stems.as_mut().unwrap().notes.push(StemNote {
        stem: StemKind::Vocals,
        start_sec: 0.,
        end_sec: 32.,
        midi: 60,
        confidence: 0.9,
    });
    assert!(!crate::foreground::preserved(&p, &ctx(&a, &b)));
    // Muting only bass leaves exactly the same incomplete vocal entrance.
    p.stem_mix.as_mut().unwrap().incoming[3] = Polyline::constant(0.);
    assert!(!crate::foreground::preserved(&p, &ctx(&a, &b)));
}

#[test]
fn a_quiet_held_tail_cannot_be_faded_after_the_main_voice_activity_drops() {
    let (mut a, b, mut p) = fixture(1.);
    arrange(&mut p, &ctx(&a, &b), Tonality::Compatible, duration_reward).unwrap();
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
    arrange(
        &mut safe,
        &ctx(&a, &b),
        Tonality::Compatible,
        duration_reward,
    )
    .unwrap();
    assert!(crate::foreground::preserved(&safe, &ctx(&a, &b)));
}

#[test]
fn incoming_words_are_not_revealed_through_a_recovering_channel_eq() {
    for held_only in [false, true] {
        let (mut a, mut b, mut p) = fixture(1.);
        for f in &mut a.stems.as_mut().unwrap().frames {
            f.vocal_activity = 0.;
            f.band_db[0] = [-120.; 3];
        }
        for f in &mut b.stems.as_mut().unwrap().frames {
            if (20. ..28.).contains(&f.start_sec) {
                f.vocal_activity = if held_only { 0. } else { 1. };
                f.band_db[0] = [-30., -18., -26.];
            }
        }
        if held_only {
            b.stems.as_mut().unwrap().notes = vec![StemNote {
                stem: StemKind::Vocals,
                start_sec: 20.,
                end_sec: 28.,
                midi: 60,
                confidence: 0.9,
            }];
        }
        let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
        let mut shape = [[Fade {
            start: 10.,
            end: 14.,
        }; 4]; 2];
        shape[0][0] = Fade {
            start: 8.,
            end: 8.125,
        };
        shape[1][0] = Fade {
            start: 8.5,
            end: 8.625,
        };
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert!(p.lanes.eq_b.mid.sample(10.) > -0.05);
        assert!(p.lanes.eq_b.high.sample(10.) > -0.05);
        assert!(crate::foreground::preserved(&p, &ctx(&a, &b)));
        // A singer already present in full may receive ordinary level
        // restoration. It does not hide the beginning of a word.
        p.lanes.eq_b.mid.nodes = vec![(0., -1.), (8., -1.), (16., 0.)];
        assert!(crate::foreground::preserved(&p, &ctx(&a, &b)));
        // Deep EQ attenuation still reveals an incomplete word, despite open
        // stem and channel faders.
        p.lanes.eq_b.mid.nodes = vec![(0., -12.), (8., -12.), (16., 0.)];
        assert!(!crate::foreground::preserved(&p, &ctx(&a, &b)));
    }
}

#[test]
fn a_loud_chord_reveal_gets_time_instead_of_an_instant_power_handoff() {
    let (a, b, mut p) = fixture(1.);
    arrange(&mut p, &ctx(&a, &b), Tonality::Compatible, duration_reward).unwrap();
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
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
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
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert_eq!(
            p.summary.as_ref().unwrap().strategy,
            StrategyId::PhraseBlend
        );
        assert_eq!(p.stages[1].label, "Fade between tracks");
        shape = [[Fade { start: 8., end: 8. }; 4]; 2];
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
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
    render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
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
    render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
    assert_eq!(
        p.summary.as_ref().unwrap().strategy,
        StrategyId::PhraseBlend
    );
}

fn duration_reward(bars: f32) -> f32 {
    0.24 * (1. - (-bars / 20.).exp()) / 1.4
}

#[test]
fn a_silent_voice_does_not_authorize_cutting_a_sustained_synth() {
    for scale in [0.25, 1., 2.] {
        let (mut a, mut b, mut p) = fixture(scale);
        for t in [&mut a, &mut b] {
            t.phrase_boundaries = vec![mixless_protocol::PhraseBoundary {
                time_sec: 16.,
                confidence: 0.9,
                novelty: 0.5,
            }];
        }
        let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
        let mut shape = [[Fade {
            start: 7.75,
            end: 8.,
        }; 4]; 2];
        shape[1] = [Fade {
            start: 8.,
            end: 8.25,
        }; 4];
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert!(
            !crate::melody::preserved(&p, &ctx(&a, &b)),
            "a half-second melody cut is not a phrase exchange"
        );
        shape[0][2] = Fade { start: 4., end: 8. };
        shape[1][2] = Fade {
            start: 8.,
            end: 12.,
        };
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert!(
            crate::melody::preserved(&p, &ctx(&a, &b)),
            "a gradual phrase release remains available"
        );
        shape[0][2] = Fade {
            start: 10.,
            end: 14.,
        };
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert!(
            !crate::melody::preserved(&p, &ctx(&a, &b)),
            "slow fades still end on a musical release"
        );
        for t in [&mut a, &mut b] {
            for f in &mut t.stems.as_mut().unwrap().frames {
                f.band_db[2][1] = -120.;
                f.band_db[2][2] = -120.;
            }
        }
        let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
        shape[0][2] = Fade {
            start: 7.75,
            end: 8.,
        };
        shape[1][2] = Fade {
            start: 8.,
            end: 8.25,
        };
        render::apply(&mut p, &frames, &shape, &ctx(&a, &b)).unwrap();
        assert!(
            crate::melody::preserved(&p, &ctx(&a, &b)),
            "absent melody may switch quietly"
        );
    }
}

#[test]
fn both_decks_need_a_sustained_selection_not_just_different_fade_widths() {
    for scale in [0.25, 1., 2.] {
        let (a, b, p) = fixture(scale);
        let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
        let base = [[Fade { start: 6., end: 8. }; 4]; 2];
        let strategy = |shape: &Shape| description::describe(&frames, shape, 16.).0;
        let mut shape = base;
        shape[0][1].end += 0.001;
        assert_eq!(strategy(&shape), StrategyId::PhraseBlend);
        // Independent fades still expose all parts on both decks.
        shape[0][1].end = 9.;
        shape[1][1].end = 9.;
        assert_eq!(strategy(&shape), StrategyId::PhraseBlend);
        // A partial incoming deck layered under the complete outgoing track.
        shape = base;
        shape[1][1] = Fade { start: 2., end: 3. };
        assert_eq!(strategy(&shape), StrategyId::PhraseBlend);
        // The same kit now replaces A's kit: both decks hold a proper subset.
        shape[0][1] = shape[1][1];
        assert_eq!(strategy(&shape), StrategyId::StemSwap);
        assert!(occupancy::measure(&frames, &shape, 16.).selected_run >= 3.);
        // A momentary stagger within the fade is not a sustained arrangement.
        shape[0][1] = Fade {
            start: 5.5,
            end: 6.,
        };
        shape[1][1] = shape[0][1];
        assert_eq!(strategy(&shape), StrategyId::PhraseBlend);
    }
}

#[test]
fn waiting_and_finished_playback_do_not_earn_overlap_duration() {
    let (a, b, p) = fixture(1.);
    let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
    let short = [[Fade {
        start: 11.,
        end: 12.,
    }; 4]; 2];
    let long = [[Fade {
        start: 2.,
        end: 14.,
    }; 4]; 2];
    let brief = occupancy::measure(&frames, &short, 16.).overlap_bars;
    let sustained = occupancy::measure(&frames, &long, 16.).overlap_bars;
    assert!(brief > 0. && brief <= 1.);
    assert!(sustained > 4. * brief);
    assert!(duration_reward(brief) < duration_reward(sustained));
    let cut = [[Fade { start: 8., end: 8. }; 4]; 2];
    assert_eq!(occupancy::measure(&frames, &cut, 16.).overlap_bars, 0.);
}

#[test]
fn an_absent_incoming_tonal_bed_does_not_manufacture_a_vocal_ride() {
    let (a, mut b, p) = fixture(1.);
    for f in &mut b.stems.as_mut().unwrap().frames {
        f.band_db[2][1] = -120.;
        f.band_db[2][2] = -120.;
    }
    let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
    let mut shape = [[Fade { start: 6., end: 8. }; 4]; 2];
    shape[0][0] = Fade {
        start: 11.,
        end: 11.25,
    };
    assert_ne!(
        description::describe(&frames, &shape, 16.).0,
        StrategyId::VocalRide
    );
}

#[test]
fn an_opening_sung_line_can_arrive_whole_over_an_instrumental_bed() {
    let (mut a, mut b, p) = fixture(1.);
    for f in &mut a.stems.as_mut().unwrap().frames {
        f.vocal_activity = 0.;
        f.rms[0] = 0.;
        f.band_db[0] = [-120.; 3];
    }
    for f in &mut b.stems.as_mut().unwrap().frames {
        f.vocal_activity = 1.;
        f.rms[0] = 0.1;
        f.band_db[0] = [-30., -24., -36.];
    }
    let mut opening = p.clone();
    arrange(
        &mut opening,
        &ctx(&a, &b),
        Tonality::Compatible,
        duration_reward,
    )
    .unwrap();
    assert!(heard(&opening, 1, 0, 0.) > 0.9);
    assert!(crate::foreground::preserved(&opening, &ctx(&a, &b)));
    // An internal source start cannot borrow the recording's natural onset.
    let mut partial = p.clone();
    partial.t_in_b = 8.;
    partial.t_end_b += 8.;
    for (_, sec) in &mut partial.incoming_source.nodes {
        *sec += 8.;
    }
    assert!(arrange(
        &mut partial,
        &ctx(&a, &b),
        Tonality::Compatible,
        duration_reward
    )
    .is_none());
    // Both singers continuously present still rule out this arrangement.
    for f in &mut a.stems.as_mut().unwrap().frames {
        f.vocal_activity = 1.;
        f.rms[0] = 0.1;
        f.band_db[0] = [-30., -24., -36.];
    }
    assert!(arrange(
        &mut p.clone(),
        &ctx(&a, &b),
        Tonality::Compatible,
        duration_reward
    )
    .is_none());
}

#[test]
fn a_parked_incoming_deck_cannot_fill_the_outgoing_bed() {
    let (mut a, b, mut p) = fixture(1.);
    for f in &mut a.stems.as_mut().unwrap().frames {
        f.vocal_activity = 0.;
        f.band_db[0] = [-120.; 3];
    }
    p.incoming_start_bar = 8.;
    p.incoming_source.nodes = vec![(0., 0.), (8., 0.), (16., 16.)];
    let fs = evidence::frames(&p, &ctx(&a, &b)).unwrap();
    assert!(fs.iter().filter(|f| f.u < 8.).all(|f| f.bands[1]
        .iter()
        .flatten()
        .all(|power| *power == 0.)
        && f.voice[1] == 0.));
    assert!(fs
        .iter()
        .filter(|f| f.u >= 8.)
        .all(|f| f.bands[1].iter().flatten().sum::<f32>() > 0.));
    let early = [[Fade { start: 2., end: 4. }; 4]; 2];
    assert!(cost(&fs, &early, Tonality::Compatible).is_none());
    let aligned = [[Fade {
        start: 8.,
        end: 10.,
    }; 4]; 2];
    assert!(cost(&fs, &aligned, Tonality::Compatible).is_some());
}

#[test]
fn cached_shape_costs_preserve_successes_and_rejections() {
    let (a, b, p) = fixture(1.);
    let frames = evidence::frames(&p, &ctx(&a, &b)).unwrap();
    for tonality in [Tonality::Compatible, Tonality::Unknown, Tonality::Conflict] {
        let cached = search::Costs::new(&frames, tonality);
        for i in 0..80 {
            let shape = std::array::from_fn(|d| {
                std::array::from_fn(|s| {
                    let start = ((i * (s + 1) + d * 3) % 60) as f32 / 4.;
                    Fade {
                        start,
                        end: (start + (s + 1) as f32 / 4.).min(16.),
                    }
                })
            });
            let expected = cost(&frames, &shape, tonality).map(f32::to_bits);
            for _ in 0..2 {
                assert_eq!(cached.get(&shape).map(f32::to_bits), expected);
            }
        }
    }
}
