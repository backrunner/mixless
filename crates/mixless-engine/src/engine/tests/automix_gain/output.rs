use super::*;

#[test]
fn automix_gain_measures_correlated_sum_and_leaves_cancellation_alone() {
    for inverted in [false, true] {
        for outgoing in [DeckId::A, DeckId::B] {
            let build = |enabled| {
                fixture_audio(
                    false,
                    None,
                    enabled,
                    outgoing,
                    |p| p.lanes.xfader = Polyline::constant(0.),
                    |d, audio| {
                        for (i, pair) in audio.samples.chunks_exact_mut(2).enumerate() {
                            let x = 0.1
                                * (std::f32::consts::TAU * 440. * i as f32 / SR as f32).sin()
                                * if inverted && d == 1 { -1. } else { 1. };
                            pair.fill(x);
                        }
                    },
                )
            };
            let on = build(true);
            let off = build(false);
            on.render_offline(4 * SR as usize);
            off.render_offline(4 * SR as usize);
            let snap = on.snapshot();
            let compensated = energy(&on.render_offline(SR as usize));
            let baseline = energy(&off.render_offline(SR as usize));
            if inverted {
                assert!(snap.decks.iter().all(|d| d.automix_gain_db <= 0.));
                assert!((compensated - baseline).abs() < 1e-8);
            } else {
                assert!(
                    snap.decks.iter().all(|d| d.automix_gain_db < -0.5),
                    "{:?}",
                    snap.decks.map(|d| d.automix_gain_db)
                );
                assert!(compensated < baseline * 0.8);
                for engine in [&on, &off] {
                    engine
                        .dispatch(Command::SetDeckLimiterGain {
                            deck: outgoing,
                            db: -12.,
                        })
                        .unwrap();
                    engine.render_offline(SR as usize / 10);
                }
                let routed_power = |engine: &Engine| {
                    let mut rt = engine.rt.lock().unwrap();
                    let mut out = [0.; 512];
                    let mut observed = [[0.; 8]; 256];
                    engine.shared.process_block_observed::<true>(
                        &mut rt,
                        &mut out,
                        2,
                        &mut observed,
                    );
                    observed
                        .iter()
                        .map(|s| s[outgoing.index() * 2].powi(2))
                        .sum::<f32>()
                };
                // A slow negative AUTO release cannot pull a manual -12 dB
                // target below the channel's supported drive range.
                assert!(routed_power(&on) >= routed_power(&off) * 0.99);
            }
        }
    }
}

#[test]
fn automix_gain_preserves_plan_and_master_edits_cancel_only_compensation() {
    let mut original = String::new();
    let engine = fixture_plan(true, None, true, DeckId::B, |p| {
        original = format!("{p:?}");
    });
    let snapshot = engine.snapshot();
    let (outgoing, plan) = engine.automix_plan(snapshot.automix_plan_id).unwrap();
    assert_eq!(outgoing, DeckId::B);
    assert_eq!(format!("{plan:?}"), original);
    engine.render_offline(4 * SR as usize);
    assert!(engine.snapshot().decks[0].automix_gain_db > 0.);
    engine.dispatch(Command::SetMasterGain { db: -2. }).unwrap();
    engine.render_offline(SR as usize);
    let snapshot = engine.snapshot();
    assert!(snapshot.automix_on);
    assert!(snapshot.decks.iter().all(|d| d.automix_gain_db == 0.));
    assert_eq!(
        format!(
            "{:?}",
            engine.automix_plan(snapshot.automix_plan_id).unwrap().1
        ),
        original
    );
}

#[test]
fn observed_playback_is_sample_identical_with_stems_pitch_filters_fx_and_master() {
    let build = || {
        let engine = fixture_plan(true, None, true, DeckId::B, |p| {
            p.lanes.pitch_b = Polyline::constant(2.);
            p.lanes.filter_a.lp_hz = line(&[(0., 20000.), (8., 1500.), (16., 20000.)]);
            p.lanes.rate_b = Polyline::constant(1.05);
            p.incoming_source = line(&[(0., 0.), (16., 8.4)]);
            p.t_end_b = 8.4;
            p.lanes.fx_send_a = line(&[(0., 0.), (4., 0.2), (12., 0.2), (16., 0.)]);
        });
        let mut settings = mixless_protocol::MasterChainSettings::default();
        settings.eq_enabled = true;
        settings.eq[2].gain_db = 2.;
        settings.compressor.enabled = true;
        settings.compressor.threshold_db = -24.;
        engine
            .dispatch(Command::SetMasterChain { settings })
            .unwrap();
        engine.dispatch(Command::SetMaster { value: 0.4 }).unwrap();
        let plan = engine
            .automix_plan(engine.snapshot().automix_plan_id)
            .unwrap()
            .1;
        engine.load_plan_on((*plan).clone(), DeckId::B).unwrap();
        engine
    };
    let a = build();
    let b = build();
    let mut ra = a.rt.lock().unwrap();
    let mut rb = b.rt.lock().unwrap();
    let mut plain = [0.; 512];
    let mut observed = [0.; 512];
    let mut capture = [[0.; 8]; 256];
    for _ in 0..(11 * SR as usize / 256) {
        a.shared.process_block(&mut ra, &mut plain, 2);
        b.shared
            .process_block_observed::<true>(&mut rb, &mut observed, 2, &mut capture);
        assert_eq!(plain, observed);
        assert!(capture.iter().flatten().all(|x| x.is_finite()));
    }
}

#[test]
fn prepared_gain_is_invalidated_by_changes_before_publication() {
    let engine = fixture(true, None, true, DeckId::A);
    let plan = engine
        .automix_plan(engine.snapshot().automix_plan_id)
        .unwrap()
        .1;
    let prepared = engine.prepare_plan_on((*plan).clone(), DeckId::A).unwrap();
    engine.dispatch(Command::SetMasterGain { db: -3. }).unwrap();
    engine.commit_plan(prepared).unwrap();
    engine.render_offline(4 * SR as usize);
    let snap = engine.snapshot();
    assert!(snap.automix_on);
    assert!(snap.decks.iter().all(|d| d.automix_gain_db == 0.));
}

#[test]
fn replacement_gain_requires_the_exact_prepared_pcm_and_stems() {
    for same_source in [true, false] {
        let engine = fixture(true, None, true, DeckId::A);
        let plan = engine
            .automix_plan(engine.snapshot().automix_plan_id)
            .unwrap()
            .1;
        let incoming = &engine.shared.decks[1];
        let original = incoming.buffer.lock().unwrap().clone().unwrap();
        let stems = incoming
            .stems
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .audio
            .clone();
        let prepared = engine
            .prepare_replacement(
                (*plan).clone(),
                DeckId::A,
                original.clone(),
                Some(stems.clone()),
                &Default::default(),
            )
            .unwrap();
        let audio = if same_source {
            original
        } else {
            sine_buffer(SR, 550., 18.)
        };
        let wave = Arc::new(crate::compute_preview_waveform(&audio, 256));
        engine
            .load_buffer_if(
                DeckId::B,
                TrackId(2),
                audio.clone(),
                wave,
                String::new(),
                String::new(),
                || true,
            )
            .unwrap();
        engine
            .attach_stems(DeckId::B, TrackId(2), &audio, stems)
            .unwrap();
        engine.commit_plan(prepared).unwrap();
        engine.render_offline(4 * SR as usize);
        let snapshot = engine.snapshot();
        assert!(snapshot.automix_on);
        if same_source {
            assert!(snapshot.decks[1].automix_gain_db > 0.);
        } else {
            assert!(snapshot.decks.iter().all(|d| d.automix_gain_db == 0.));
        }
    }
}

#[test]
fn full_tracks_with_different_local_levels_get_a_smoother_handoff() {
    let build = |enabled| {
        fixture_audio(
            false,
            None,
            enabled,
            DeckId::A,
            |p| p.lanes.xfader = line(&[(0., -1.), (7.5, -1.), (8.5, 1.), (16., 1.)]),
            |d, audio| {
                if d == 1 {
                    for s in &mut audio.samples {
                        *s *= 0.25;
                    }
                }
            },
        )
    };
    let on = build(true);
    let off = build(false);
    let measure = |engine: &Engine| {
        let mut meter = ebur128::EbuR128::new(2, SR, ebur128::Mode::M).unwrap();
        let mut loudness = Vec::new();
        let mut applied = 0f32;
        for _ in 0..40 {
            meter
                .add_frames_f32(&engine.render_offline(SR as usize / 5))
                .unwrap();
            loudness.push(meter.loudness_momentary().unwrap());
            applied = engine
                .snapshot()
                .decks
                .iter()
                .map(|d| d.automix_gain_db.abs())
                .fold(applied, f32::max);
        }
        let step = loudness[15..26]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0f64, f64::max);
        (step, applied)
    };
    let corrected = measure(&on);
    let baseline = measure(&off);
    assert!(corrected.1 > 0.5, "{corrected:?} {baseline:?}");
    assert!(corrected.0 < baseline.0 - 0.2, "{corrected:?} {baseline:?}");
}
