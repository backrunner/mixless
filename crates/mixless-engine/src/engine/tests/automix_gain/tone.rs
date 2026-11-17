use super::*;

fn component(frame: usize, stem: usize) -> f32 {
    let frequency = [700., 3800., 1300., 70.][stem];
    0.07 * (std::f32::consts::TAU * frequency * frame as f32 / SR as f32).sin()
}

fn multiband(stem: Option<usize>, band: Option<usize>, enabled: bool, outgoing: DeckId) -> Engine {
    let engine = fixture_audio(
        false,
        None,
        true,
        outgoing,
        |_| {},
        |_, audio| {
            for (i, frame) in audio.samples.chunks_exact_mut(2).enumerate() {
                frame.fill((0..4).map(|s| component(i, s)).sum());
            }
        },
    );
    let mut plan = (*engine
        .automix_plan(engine.snapshot().automix_plan_id)
        .unwrap()
        .1)
        .clone();
    engine.dispatch(Command::StopAutomix).unwrap();
    plan.lanes.xfader = Polyline::constant(1.);
    if let Some(band) = band {
        let lane = match band {
            0 => &mut plan.lanes.eq_b.low,
            1 => &mut plan.lanes.eq_b.mid,
            _ => &mut plan.lanes.eq_b.high,
        };
        *lane = Polyline::constant(-8.);
    }
    if let Some(stem) = stem {
        for deck in [DeckId::A, DeckId::B] {
            let slot = &engine.shared.decks[deck.index()];
            let audio = slot.buffer.lock().unwrap().clone().unwrap();
            let samples = |s| {
                (0..audio.frames as usize)
                    .flat_map(|i| [component(i, s); 2])
                    .collect()
            };
            let stems = Arc::new(
                crate::StemBuffer::with_bass(SR, samples(0), samples(1), samples(3)).unwrap(),
            );
            engine
                .attach_stems(
                    deck,
                    TrackId(slot.track_id.load(Ordering::Relaxed) as i64),
                    &audio,
                    stems,
                )
                .unwrap();
        }
        plan.stem_mix = Some(StemMix {
            outgoing: std::array::from_fn(|_| Polyline::constant(1.)),
            incoming: std::array::from_fn(|i| Polyline::constant(if i == stem { 0.4 } else { 1. })),
        });
    }
    engine.load_plan_on(plan, outgoing).unwrap();
    if !enabled {
        engine
            .dispatch(Command::SetDeckLimiterGain {
                deck: outgoing,
                db: 0.,
            })
            .unwrap();
    }
    engine
}

fn bands(engine: &Engine) -> [f64; 3] {
    let mut filters = [Isolator::new(SR as f32), Isolator::new(SR as f32)];
    let mut power = [0.; 3];
    for (i, frame) in engine
        .render_offline(SR as usize)
        .chunks_exact(2)
        .enumerate()
    {
        for ch in 0..2 {
            let split = filters[ch].split(frame[ch]);
            if i > SR as usize / 10 {
                for b in 0..3 {
                    power[b] += f64::from(split[b]).powi(2);
                }
            }
        }
    }
    power
}

#[test]
fn automix_tone_independently_repairs_each_eq_band_without_broadband_overcompensation() {
    for band in 0..3 {
        let on = multiband(None, Some(band), true, DeckId::A);
        let off = multiband(None, Some(band), false, DeckId::A);
        for e in [&on, &off] {
            e.render_offline(4 * SR as usize);
        }
        let snapshot = on.snapshot();
        let correction = snapshot.decks[1].automix_eq_db;
        assert!(correction[band] > 0.3, "band {band}: {correction:?}");
        let a = bands(&on);
        let b = bands(&off);
        let neutral = multiband(None, None, false, DeckId::A);
        neutral.render_offline(4 * SR as usize);
        let target = bands(&neutral);
        assert!(10. * (a[band] / b[band]).log10() > 0.3, "{a:?} {b:?}");
        for other in 0..3 {
            if other != band {
                assert!(
                    10. * (a[other] / target[other]).log10() < 1.05,
                    "{a:?} {target:?}"
                );
            }
        }
    }
}

#[test]
fn automix_tone_can_correct_each_retained_stem_and_manual_takeover_clears_the_layer() {
    for stem in 0..4 {
        let on = multiband(Some(stem), None, true, DeckId::B);
        on.render_offline(4 * SR as usize);
        let snapshot = on.snapshot();
        let d = &snapshot.decks[0];
        assert!(
            d.automix_stem_db[stem] > 0.,
            "stem {stem}: {:?} {:?}",
            d.automix_stem_db,
            d.automix_eq_db
        );
        assert!(d.stem_gain[stem] > 0.4 && d.stem_gain[stem] <= 1.);
        let value = d.stem_gain[stem];
        on.dispatch(Command::SetStemGain {
            deck: DeckId::A,
            stem: [
                mixless_protocol::StemKind::Vocals,
                mixless_protocol::StemKind::Drums,
                mixless_protocol::StemKind::Instruments,
                mixless_protocol::StemKind::Bass,
            ][stem],
            value,
        })
        .unwrap();
        on.render_offline(SR as usize / 4);
        let snapshot = on.snapshot();
        assert!(snapshot.automix_on);
        assert!((snapshot.decks[0].stem_gain[stem] - value).abs() < 0.002);
        for d in snapshot.decks {
            assert_eq!(d.automix_eq_db, [0.; 3]);
            assert_eq!(d.automix_stem_db, [0.; 4]);
            assert_eq!(d.automix_gain_db, 0.);
        }
    }
}

#[test]
fn automix_tone_keeps_fade_direction_silence_plan_and_combined_budget() {
    let engine = multiband(Some(3), None, true, DeckId::A);
    let mut plan = (*engine
        .automix_plan(engine.snapshot().automix_plan_id)
        .unwrap()
        .1)
        .clone();
    plan.stem_mix.as_mut().unwrap().incoming[3] =
        line(&[(0., 0.4), (6., 0.4), (12., 0.), (16., 0.)]);
    let original = format!("{plan:?}");
    engine.load_plan_on(plan, DeckId::A).unwrap();
    let mut last = 1.;
    for i in 0..40 {
        engine.render_offline(SR as usize / 5);
        let snapshot = engine.snapshot();
        if (16..30).contains(&i) {
            assert!(snapshot.decks[1].stem_gain[3] <= last + 0.001);
        }
        last = snapshot.decks[1].stem_gain[3];
        if (31..38).contains(&i) {
            assert_eq!(last, 0.);
        }
        for deck in snapshot.decks {
            for eq in deck.automix_eq_db {
                for stem in deck.automix_stem_db {
                    let path = [deck.automix_gain_db, eq, stem];
                    assert!(path.iter().map(|x| x.max(0.)).sum::<f32>() <= 6.02);
                    assert!(path.iter().map(|x| x.min(0.)).sum::<f32>() >= -3.02);
                }
            }
        }
    }
    let published = engine
        .automix_plan(engine.snapshot().automix_plan_id)
        .unwrap()
        .1;
    assert_eq!(format!("{published:?}"), original);
    engine.render_offline(SR as usize);
    for deck in engine.snapshot().decks {
        assert_eq!(deck.automix_eq_db, [0.; 3]);
        assert_eq!(deck.automix_stem_db, [0.; 4]);
    }
}

#[test]
fn automix_tone_respects_kill_and_cancellation_without_callback_allocations() {
    let engine = multiband(None, Some(0), true, DeckId::A);
    engine.render_offline(4 * SR as usize);
    assert!(engine.snapshot().decks[1].automix_eq_db[0] > 0.);
    let mut rt = engine.rt.lock().unwrap();
    let mut output = [0.; 512];
    assert_eq!(
        super::super::allocation::measure(|| engine.shared.process_block(&mut rt, &mut output, 2)),
        [0; 3]
    );
    drop(rt);
    engine
        .dispatch(Command::SetEqKill {
            deck: DeckId::B,
            band: mixless_protocol::EqBand::Low,
            on: true,
        })
        .unwrap();
    engine.render_offline(SR as usize / 100);
    {
        let rt = engine.rt.lock().unwrap();
        assert_eq!(rt.decks[1].eq[0].current(), 0.);
    }
    let snapshot = engine.snapshot();
    assert!(snapshot.automix_on && snapshot.decks[1].eq_kill[0]);
    assert!(snapshot
        .decks
        .iter()
        .all(|d| d.automix_eq_db == [0.; 3] && d.automix_stem_db == [0.; 4]));
}

#[test]
fn automix_tone_can_attenuate_excess_in_a_retained_stem() {
    let engine = multiband(Some(3), None, true, DeckId::A);
    let mut plan = (*engine
        .automix_plan(engine.snapshot().automix_plan_id)
        .unwrap()
        .1)
        .clone();
    plan.stem_mix.as_mut().unwrap().incoming[3] = Polyline::constant(1.);
    plan.lanes.eq_b.low = Polyline::constant(6.);
    engine.load_plan_on(plan, DeckId::A).unwrap();
    engine.render_offline(4 * SR as usize);
    let snapshot = engine.snapshot();
    let d = &snapshot.decks[1];
    assert!(
        d.automix_stem_db[3] < -0.1,
        "{:?} {:?}",
        d.automix_stem_db,
        d.automix_eq_db
    );
    assert!(d.stem_gain[3] < 1. && d.stem_gain[3] > 0.);
    assert!(d.automix_eq_db.iter().all(|v| *v <= 0.));
}

#[test]
fn automix_tone_eq_takeover_does_not_apply_the_overlay_twice() {
    let engine = multiband(None, Some(0), true, DeckId::A);
    engine.render_offline(4 * SR as usize);
    let d = engine.snapshot().decks[1].clone();
    assert!(d.automix_eq_db[0] > 0.);
    engine
        .dispatch(Command::SetEq {
            deck: DeckId::B,
            band: mixless_protocol::EqBand::Low,
            db: d.eq_db[0],
        })
        .unwrap();
    for _ in 0..8 {
        engine.render_offline(64);
        let rt = engine.rt.lock().unwrap();
        let actual = rt.decks[1].eq[0].current() * rt.decks[1].automix_eq[0].current();
        assert!(20. * actual.log10() <= d.eq_db[0] + 0.02);
    }
}

#[test]
fn automix_tone_master_edit_preserves_release_over_a_moving_eq_lane() {
    let engine = multiband(None, Some(0), true, DeckId::A);
    let mut plan = (*engine
        .automix_plan(engine.snapshot().automix_plan_id)
        .unwrap()
        .1)
        .clone();
    plan.lanes.eq_b.low = line(&[(0., -8.), (16., -10.)]);
    engine.load_plan_on(plan, DeckId::A).unwrap();
    engine.render_offline(4 * SR as usize);
    let before = engine.rt.lock().unwrap().decks[1].automix_eq[0].current();
    assert!(before > 1.01);
    engine.dispatch(Command::SetMasterGain { db: -1. }).unwrap();
    engine.render_offline(SR as usize / 100);
    let during = engine.rt.lock().unwrap().decks[1].automix_eq[0].current();
    assert!(during > 1. && during < before);
    engine.render_offline(SR as usize / 4);
    assert_eq!(
        engine.rt.lock().unwrap().decks[1].automix_eq[0].current(),
        1.
    );
    assert!(engine.snapshot().automix_on);
}
