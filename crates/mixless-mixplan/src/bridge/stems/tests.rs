use super::*;
use mixless_protocol::{MixPlanSummary, SectionLabel, StemKind, StemNote, TransitionMode};

#[test]
fn shared_powers_preserve_original_sampling_and_float_operations() {
    fn original(t: &TrackAnalysis, sec: f32, width: f32) -> Option<([[f32; 3]; 4], f32, [f32; 3])> {
        let meters = t.stems.as_ref()?;
        let mut bands = [[0.; 3]; 4];
        let mut voice = 0f32;
        let mut onset = [0f32; 3];
        for i in 0..5 {
            let s = (sec + (i as f32 / 4. - 0.5) * width).clamp(0., t.duration_sec - 0.001);
            let f = meters.frame_at(s)?;
            voice = voice.max(f.vocal_activity);
            for part in 0..3 {
                onset[part] = onset[part].max(f.onset[part]);
                for band in 0..3 {
                    let target = if part == 2 && band == 0 { 3 } else { part };
                    bands[target][band] += 10f32.powf(f.band_db[part][band] / 10.) / 5.;
                }
            }
        }
        Some((bands, voice, onset))
    }
    let (mut a, b, _) = fixture();
    for (i, f) in a.stems.as_mut().unwrap().frames.iter_mut().enumerate() {
        for (j, db) in f.band_db.iter_mut().flatten().enumerate() {
            *db = -90. + ((i * 17 + j * 23) % 900) as f32 / 10.;
        }
    }
    a.stems.as_mut().unwrap().frames.remove(17);
    let mut missing = b.clone();
    missing.stems = None;
    let bits = |(power, voice, onset): ([[f32; 3]; 4], f32, [f32; 3])| {
        (
            power.map(|p| p.map(f32::to_bits)),
            voice.to_bits(),
            onset.map(f32::to_bits),
        )
    };
    for scoped in [false, true] {
        let evidence = crate::level::evidence::Evidence::new(&a, &b);
        let _scope = scoped.then(|| evidence.enter());
        for t in [&a, &b, &missing] {
            for sec in [-1., 0., 0.125, 2., 4.25, 20., t.duration_sec, f32::NAN] {
                for width in [0., 0.2, 1., 3.] {
                    assert_eq!(
                        powers(t, sec, width).map(bits),
                        original(t, sec, width).map(bits),
                        "scoped={scoped}, time={sec}, width={width}"
                    );
                }
            }
        }
    }
}

fn fixture() -> (TrackAnalysis, TrackAnalysis, MixPlan) {
    let mut a = crate::stem_regressions::with_stems(crate::tests::track(
        1,
        100.,
        "8A",
        SectionLabel::Outro,
        64,
        0.8,
        0.3,
    ));
    let mut b = crate::stem_regressions::with_stems(crate::tests::track(
        2,
        178.,
        "8A",
        SectionLabel::Intro,
        64,
        0.8,
        0.3,
    ));
    for t in [&mut a, &mut b] {
        for f in &mut t.stems.as_mut().unwrap().frames {
            f.vocal_activity = 0.;
            f.band_db[0] = [-65.; 3];
        }
        for bar in &mut t.bars {
            bar.vocal_presence = 0.;
            bar.vocal_confidence = Some(0.);
        }
    }
    let line = |start, end| Polyline {
        nodes: vec![(0., start), (4., end)],
    };
    let mut p = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: StrategyId::PhraseBlend,
            score: 0.8,
            used_fallback: false,
            length_bars: 4,
        }),
        clock: line(0., 9.6),
        outgoing_source: line(0., 9.6),
        incoming_source: line(0., 9.6),
        t_in_a: 0.,
        t_out_a: 9.6,
        t_in_b: 0.,
        t_end_b: 9.6,
        transition_mode: Some(TransitionMode::PhraseBridge),
        ..Default::default()
    };
    p.lanes.rate_a = Polyline::constant(1.);
    p.lanes.rate_b = Polyline::constant(1.);
    (a, b, p)
}
fn run(a: &TrackAnalysis, b: &TrackAnalysis, p: &mut MixPlan, harmonic: bool) -> Option<()> {
    arrange(
        p,
        &PlanContext {
            outgoing: a,
            incoming: b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        },
        harmonic,
    )
}

#[test]
fn separated_rhythm_and_keys_have_exact_boundaries_and_neutral_endpoints() {
    for harmonic in [false, true] {
        let (a, b, mut p) = fixture();
        run(&a, &b, &mut p, harmonic).unwrap();
        let s = p.stem_mix.as_ref().unwrap();
        for part in 0..4 {
            assert_eq!(s.outgoing[part].sample(0.), 1.);
            assert_eq!(s.incoming[part].sample(0.), 0.);
            assert_eq!(s.outgoing[part].sample(4.), 0.);
            assert_eq!(s.incoming[part].sample(4.), 1.);
            for (d, line, beat) in [
                (0, &s.outgoing[part], 0.6),
                (1, &s.incoming[part], 60. / 178.),
            ] {
                for w in line.nodes.windows(2) {
                    let dt = p.clock.sample(w[1].0) - p.clock.sample(w[0].0);
                    // The crossfader now carries the common envelope. Test
                    // the routed amplitude, not a silent channel's stem knob.
                    let heard = |u: f32, stem: f32| {
                        let x = p.lanes.xfader.sample(u);
                        let cross = if d == 0 {
                            ((1. - x) / 2.).sqrt()
                        } else {
                            ((1. + x) / 2.).sqrt()
                        };
                        let gain = if d == 0 {
                            p.lanes.gain_a.sample(u)
                        } else {
                            p.lanes.gain_b.sample(u)
                        };
                        stem * cross * 10f32.powf(gain / 20.)
                    };
                    assert!(
                        (heard(w[1].0, w[1].1) - heard(w[0].0, w[0].1)).abs() * beat
                            <= dt * 1.2 + 0.011
                    );
                }
            }
        }
        for i in 0..=4096 {
            let u = 4. * i as f32 / 4096.;
            for part in [1, 3] {
                assert!(s.outgoing[part].sample(u) * s.incoming[part].sample(u) <= 0.0001);
            }
            if !harmonic {
                let a = [0, 2, 3]
                    .map(|s0| s.outgoing[s0].sample(u))
                    .into_iter()
                    .fold(0., f32::max);
                let b = [0, 2, 3]
                    .map(|s0| s.incoming[s0].sample(u))
                    .into_iter()
                    .fold(0., f32::max);
                assert!(a * b <= 0.0001);
            }
        }
        assert_eq!(p.lanes.gain_a.sample(0.), 0.);
        assert_eq!(p.lanes.gain_a.sample(4.), -96.);
        assert_eq!(p.lanes.gain_b.sample(4.), 0.);
        for eq in [&p.lanes.eq_a, &p.lanes.eq_b] {
            for line in [&eq.low, &eq.mid, &eq.high] {
                assert_eq!(line.sample(0.), 0.);
                assert_eq!(line.sample(4.), 0.);
                for w in line.nodes.windows(2) {
                    let dt = p.clock.sample(w[1].0) - p.clock.sample(w[0].0);
                    assert!((w[1].1 - w[0].1).abs() <= dt + 0.0001);
                }
            }
        }
        assert!(p.handoff_bar.unwrap() > 0.5);
        assert!(crate::level::worst_dip(&p, &a, &b) < 9.);
    }
}

#[test]
fn source_energy_changes_the_schedules_without_changing_the_transition_name() {
    let (a, b, mut plain) = fixture();
    run(&a, &b, &mut plain, true).unwrap();
    let mut late = b.clone();
    for f in &mut late.stems.as_mut().unwrap().frames {
        if f.start_sec < 5. {
            f.band_db[1] = [-70.; 3];
        } else {
            f.band_db[2] = [-70.; 3];
        }
    }
    let (_, _, mut changed) = fixture();
    run(&a, &late, &mut changed, true).unwrap();
    let x = plain.stem_mix.unwrap();
    let y = changed.stem_mix.unwrap();
    assert_ne!(x.incoming[1].nodes, y.incoming[1].nodes);
    assert_ne!(x.outgoing[2].nodes, y.outgoing[2].nodes);
    assert_ne!(y.incoming[1].nodes, y.incoming[2].nodes);
}

#[test]
fn common_gain_does_not_choose_a_different_handover() {
    let (a, b, template) = fixture();
    let mut base = template.clone();
    run(&a, &b, &mut base, true).unwrap();
    for gain in [0.25f32, 2.] {
        let (mut a, mut b, mut p) = (a.clone(), b.clone(), template.clone());
        for t in [&mut a, &mut b] {
            for f in &mut t.stems.as_mut().unwrap().frames {
                for band in f.band_db.iter_mut().flatten() {
                    *band += 20. * gain.log10();
                }
                for rms in &mut f.rms {
                    *rms *= gain;
                }
            }
        }
        run(&a, &b, &mut p, true).unwrap();
        assert!((p.handoff_bar.unwrap() - base.handoff_bar.unwrap()).abs() < 1e-5);
        for (a, b) in p
            .stem_mix
            .unwrap()
            .incoming
            .iter()
            .zip(&base.stem_mix.as_ref().unwrap().incoming)
        {
            for u in [0., 0.5, 1., 2., 3., 4.] {
                assert!((a.sample(u) - b.sample(u)).abs() < 1e-4);
            }
        }
    }
}

#[test]
fn no_bed_missing_stems_and_a_crossing_sung_note_are_not_authority_to_mix() {
    let (mut a, b, mut p) = fixture();
    a.stems = None;
    assert!(run(&a, &b, &mut p, true).is_none());
    let (mut a, mut b, mut p) = fixture();
    for t in [&mut a, &mut b] {
        for f in &mut t.stems.as_mut().unwrap().frames {
            f.band_db[0] = [-100.; 3];
            f.band_db[2] = [-100.; 3];
        }
    }
    assert!(
        run(&a, &b, &mut p, true).is_none(),
        "a drum-only gap has no tonal bed"
    );
    let (mut a, b, mut p) = fixture();
    let stem = a.stems.as_mut().unwrap();
    for f in &mut stem.frames {
        f.vocal_activity = 1.;
        f.rms[0] = 0.3;
    }
    stem.notes.push(StemNote {
        stem: StemKind::Vocals,
        start_sec: 0.,
        end_sec: 20.,
        midi: 60,
        confidence: 0.99,
    });
    assert!(
        run(&a, &b, &mut p, true).is_none(),
        "never cut a sustained word to fit the bridge"
    );
}

#[test]
fn muting_drums_does_not_make_a_plucked_foreground_beatless() {
    let (mut a, mut b, p) = fixture();
    for t in [&mut a, &mut b] {
        for f in &mut t.stems.as_mut().unwrap().frames {
            f.onset = [1.; 3];
        }
    }
    assert!(run(&a, &b, &mut p.clone(), true).is_none());
    for t in [&mut a, &mut b] {
        for f in &mut t.stems.as_mut().unwrap().frames {
            f.onset[2] = 0.;
        }
    }
    assert!(run(&a, &b, &mut p.clone(), true).is_some());
}
