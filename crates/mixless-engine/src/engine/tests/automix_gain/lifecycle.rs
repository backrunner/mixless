use super::*;

#[test]
fn live_eq_gestures_preserve_armed_compensation_and_transition_lanes() {
    // Start before the armed transition, as the desktop performer does.
    let engine = fixture_plan(true, None, true, DeckId::A, |plan| {
        plan.t_in_a = 6.;
        plan.t_out_a = 14.;
        plan.outgoing_source = line(&[(0., 6.), (16., 14.)]);
    });
    let plan_id = engine.snapshot().automix_plan_id;
    let original = format!("{:?}", engine.automix_plan(plan_id).unwrap().1);
    for db in [-1., 0.] {
        engine
            .perform(Command::SetEq {
                deck: DeckId::A,
                band: mixless_protocol::EqBand::Mid,
                db,
            })
            .unwrap();
        engine.render_offline(SR as usize / 4);
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.decks[0].eq_db[1], db);
        assert!(snapshot.automix_on && snapshot.automix_progress == 0.);
    }
    engine.render_offline((5.5 * SR as f32) as usize);
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.automix_plan_id, plan_id);
    assert_eq!(snapshot.decks[0].eq_db[1], 0.);
    assert!(snapshot.automix_on && snapshot.decks[1].automix_gain_db > 1.);
    assert_eq!(
        format!("{:?}", engine.automix_plan(plan_id).unwrap().1),
        original
    );
    assert!(engine
        .perform(Command::SetEq {
            deck: DeckId::A,
            band: mixless_protocol::EqBand::Mid,
            db: f32::NAN,
        })
        .is_err());
    assert_eq!(engine.snapshot().decks[0].eq_db[1], 0.);
}

#[test]
fn manual_gain_takeover_preserves_the_audible_level() {
    for negative in [false, true] {
        let engine = if negative {
            fixture_audio(
                false,
                None,
                true,
                DeckId::A,
                |p| p.lanes.xfader = Polyline::constant(0.),
                |_, audio| {
                    for (i, frame) in audio.samples.chunks_exact_mut(2).enumerate() {
                        frame.fill(
                            0.1 * (std::f32::consts::TAU * 440. * i as f32 / SR as f32).sin(),
                        );
                    }
                },
            )
        } else {
            fixture_plan(false, None, true, DeckId::A, |p| {
                p.lanes.eq_b.mid = Polyline::constant(-9.);
                p.lanes.xfader = Polyline::constant(1.);
            })
        };
        engine.render_offline(4 * SR as usize);
        let before = engine.snapshot().decks[1].effective_limiter_gain_db();
        assert!(if negative { before < -0.5 } else { before > 1. });
        let audible_before = {
            let rt = engine.rt.lock().unwrap();
            20. * (rt.decks[1].limiter_gain.current() * rt.decks[1].automix_gain.current()).log10()
        };
        engine
            .dispatch(Command::SetDeckLimiterGain {
                deck: DeckId::B,
                db: before,
            })
            .unwrap();
        {
            let mut rt = engine.rt.lock().unwrap();
            let mut out = [0.; 64];
            assert_eq!(
                super::super::allocation::measure(|| engine
                    .shared
                    .process_block(&mut rt, &mut out, 2)),
                [0; 3]
            );
        }
        // A gesture starts at the displayed effective value. The manual and AUTO
        // smoothers must not temporarily apply that same correction twice.
        for _ in 0..100 {
            engine.render_offline(32);
            let rt = engine.rt.lock().unwrap();
            let deck = &rt.decks[1];
            let audible = 20. * (deck.limiter_gain.current() * deck.automix_gain.current()).log10();
            assert!(
                audible >= audible_before.min(before) - 0.02
                    && audible <= audible_before.max(before) + 0.02,
                "before={audible_before}, target={before}, audible={audible}"
            );
        }
    }
}

#[test]
fn signal_and_stop_edits_invalidate_prepared_compensation() {
    for cmd in [
        Command::SetFxAutoFade { on: false },
        Command::SetRoll {
            deck: DeckId::A,
            division: 4,
            on: true,
        },
        Command::StopAutomix,
    ] {
        let engine = fixture(true, None, true, DeckId::A);
        let plan = engine
            .automix_plan(engine.snapshot().automix_plan_id)
            .unwrap()
            .1;
        let prepared = engine.prepare_plan_on((*plan).clone(), DeckId::A).unwrap();
        engine.dispatch(cmd.clone()).unwrap();
        engine.commit_plan(prepared).unwrap();
        engine.render_offline(4 * SR as usize);
        assert!(
            engine
                .snapshot()
                .decks
                .iter()
                .all(|d| d.automix_gain_db == 0.),
            "{cmd:?}"
        );
    }
}

#[test]
fn active_roll_does_not_fit_a_forward_only_preview() {
    let engine = fixture(true, None, true, DeckId::A);
    let plan = engine
        .automix_plan(engine.snapshot().automix_plan_id)
        .unwrap()
        .1;
    engine
        .dispatch(Command::SetRoll {
            deck: DeckId::A,
            division: 4,
            on: true,
        })
        .unwrap();
    engine.load_plan_on((*plan).clone(), DeckId::A).unwrap();
    engine.render_offline(4 * SR as usize);
    assert!(engine
        .snapshot()
        .decks
        .iter()
        .all(|d| d.automix_gain_db == 0.));
}

#[test]
fn late_four_stem_upgrade_releases_measurements_of_the_old_stems() {
    let engine = fixture(true, None, true, DeckId::A);
    engine.render_offline(4 * SR as usize);
    assert!(engine.snapshot().decks[1].automix_gain_db > 1.);
    let source = engine.stem_source(DeckId::B, TrackId(2)).unwrap();
    let part = |level| source.samples.iter().map(|v| *v * level).collect();
    let four =
        Arc::new(crate::StemBuffer::with_bass(SR, part(0.5), part(0.25), part(0.2)).unwrap());
    assert!(engine
        .attach_stems(DeckId::B, TrackId(2), &source, four)
        .unwrap());
    engine.render_offline(SR as usize / 4);
    let snap = engine.snapshot();
    assert!(snap.automix_on && engine.four_stems_ready(DeckId::B));
    assert!(snap.decks.iter().all(|d| d.automix_gain_db == 0.
        && d.automix_eq_db == [0.; 3]
        && d.automix_stem_db == [0.; 4]));
}

#[test]
fn manual_master_edit_cannot_be_overwritten_by_compensation_publication() {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    let engine = Arc::new(fixture(true, None, true, DeckId::A));
    let plan = engine
        .automix_plan(engine.snapshot().automix_plan_id)
        .unwrap()
        .1;
    let prepared = engine.prepare_plan_on((*plan).clone(), DeckId::A).unwrap();
    // Pause publication during source verification, after its revision read.
    let stems = engine.shared.decks[0].stems.lock().unwrap();
    let publishing = {
        let engine = engine.clone();
        std::thread::spawn(move || engine.commit_plan(prepared).unwrap())
    };
    let began = Instant::now();
    loop {
        if engine.shared.decks[0].buffer.try_lock().is_err() {
            break;
        }
        assert!(began.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    let (started, ready) = mpsc::channel();
    let (completed, done) = mpsc::channel();
    let editing = {
        let engine = engine.clone();
        std::thread::spawn(move || {
            started.send(()).unwrap();
            engine.dispatch(Command::SetMasterGain { db: -3. }).unwrap();
            completed.send(()).unwrap();
        })
    };
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    // Give the old unsynchronized edit a chance to finish inside publication.
    let _ = done.recv_timeout(Duration::from_millis(100));
    drop(stems);
    publishing.join().unwrap();
    editing.join().unwrap();
    engine.render_offline(4 * SR as usize);
    let snap = engine.snapshot();
    assert!(snap.automix_on);
    assert!(
        snap.decks.iter().all(|d| d.automix_gain_db == 0.),
        "stale compensation survived the edit"
    );
}
