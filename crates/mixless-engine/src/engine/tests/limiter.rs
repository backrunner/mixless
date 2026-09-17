use super::*;

fn single_deck(gain_db: f32, trim_db: f32, fader: f32) -> Engine {
    let engine = test_engine(48_000);
    for command in [
        Command::SetCrossfader { value: -1.0 },
        Command::SetMaster { value: 1.0 },
        Command::SetCueGain { value: 1.0 },
        Command::SetChannelGain {
            deck: DeckId::A,
            db: trim_db,
        },
        Command::SetDeckLimiterGain {
            deck: DeckId::A,
            db: gain_db,
        },
        Command::SetChannelFader {
            deck: DeckId::A,
            value: fader,
        },
        Command::SetPfl {
            deck: DeckId::A,
            on: true,
        },
        Command::PlayPause { deck: DeckId::A },
    ] {
        engine.dispatch(command).unwrap();
    }
    engine.render_offline(4096);
    engine
}

#[test]
fn limiter_gain_is_independent_of_trim_and_rejects_non_finite_values() {
    let engine = single_deck(6.0, -3.0, 1.0);
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.decks[0].gain_db, -3.0);
    assert_eq!(snapshot.decks[0].limiter_gain_db, 6.0);
    assert_eq!(snapshot.decks[1].gain_db, 0.0);
    assert_eq!(snapshot.decks[1].limiter_gain_db, 0.0);

    for db in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(engine
            .dispatch(Command::SetDeckLimiterGain {
                deck: DeckId::A,
                db,
            })
            .is_err());
    }
    assert_eq!(engine.snapshot().decks[0].limiter_gain_db, 6.0);
    for (db, expected) in [(-100.0, -12.0), (100.0, 12.0)] {
        engine
            .dispatch(Command::SetDeckLimiterGain {
                deck: DeckId::A,
                db,
            })
            .unwrap();
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.decks[0].limiter_gain_db, expected);
        assert_eq!(snapshot.decks[0].gain_db, -3.0);
    }
    engine
        .dispatch(Command::SetChannelGain {
            deck: DeckId::A,
            db: -6.0,
        })
        .unwrap();
    assert_eq!(engine.snapshot().decks[0].limiter_gain_db, 12.0);
}

#[test]
fn limiter_gain_amplifies_quiet_audio_by_the_requested_db() {
    let unity = single_deck(0.0, -12.0, 1.0).render_offline(4096);
    let boosted = single_deck(6.0, -12.0, 1.0).render_offline(4096);
    assert!(energy(&unity) > 0.001);
    let gain_db = 10.0 * (energy(&boosted) / energy(&unity)).log10();
    assert!((gain_db - 6.0).abs() < 0.01, "measured {gain_db} dB");
}

#[test]
fn master_gain_is_limited_before_independent_output_level() {
    let render = |level| {
        let engine = single_deck(0., 0., 1.);
        engine.dispatch(Command::SetMasterGain { db: 12. }).unwrap();
        engine
            .dispatch(Command::SetMaster { value: level })
            .unwrap();
        engine.render_offline(4096);
        let snap = engine.snapshot();
        assert_eq!(snap.master_gain_db, 12.);
        assert_eq!(snap.master, level);
        engine.render_offline(4096)
    };
    let full = render(1.);
    let quarter = render(0.25);
    assert!(full.iter().any(|s| s.abs() > 0.97));
    for (full, quarter) in full.iter().zip(&quarter) {
        assert!(full.abs() <= 0.980001 && quarter.abs() <= 0.245001);
        assert!((full * 0.25 - quarter).abs() < 1e-6);
    }
    assert!(energy(&render(0.)) < 1e-12);
    let engine = test_engine(48000);
    for db in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(engine.dispatch(Command::SetMasterGain { db }).is_err());
    }
    assert_eq!(engine.snapshot().master_gain_db, 0.);
}

#[test]
fn deck_limiter_caps_peaks_before_fader_and_pfl() {
    let render = |fader| {
        let engine = single_deck(12.0, 0.0, fader);
        let (producer, mut consumer) = rtrb::RingBuffer::new(4096);
        let mut rt = engine.rt.lock().unwrap();
        rt.cue_output = Some(device::CueWriter::new(producer, 48_000, 48_000));
        let mut output = vec![0.0; 4096 * 2];
        engine.shared.process_block(&mut rt, &mut output, 2);
        let cue: Vec<_> = std::iter::from_fn(|| consumer.pop().ok())
            .flatten()
            .collect();
        (output, cue)
    };
    let (full, full_cue) = render(1.0);
    let (quiet, quiet_cue) = render(0.25);
    let (muted, muted_cue) = render(0.0);
    assert!(full.iter().any(|sample| sample.abs() > 0.97));
    assert!(full.iter().all(|sample| sample.abs() <= 0.980001));
    assert!(quiet.iter().all(|sample| sample.abs() <= 0.245001));
    for (full, quiet) in full.iter().zip(&quiet) {
        assert!((full * 0.25 - quiet).abs() < 1e-6);
    }
    assert_eq!(full_cue.len(), full.len());
    // CueWriter's interpolator introduces one stereo frame of delay even
    // when both devices run at the same rate.
    for (master, cue) in full.iter().zip(&full_cue[2..]) {
        assert!((master - cue).abs() < 1e-6);
    }
    assert_eq!(full_cue, quiet_cue);
    assert_eq!(full_cue, muted_cue);
    assert!(energy(&full_cue) > 0.1);
    assert!(energy(&muted) < 1e-12);
}

#[test]
fn limiting_one_deck_does_not_turn_down_the_other() {
    let render = |gain_db| {
        let engine = test_engine(48_000);
        // Separate decks into L/R, with enough master headroom to isolate
        // deck limiting from the shared output limiter.
        engine.dispatch(Command::SetMaster { value: 0.25 }).unwrap();
        for (deck, balance) in [(DeckId::A, -1.0), (DeckId::B, 1.0)] {
            engine
                .dispatch(Command::SetBalance {
                    deck,
                    value: balance,
                })
                .unwrap();
            engine.dispatch(Command::PlayPause { deck }).unwrap();
        }
        engine
            .dispatch(Command::SetDeckLimiterGain {
                deck: DeckId::A,
                db: gain_db,
            })
            .unwrap();
        engine.render_offline(4096);
        engine.render_offline(4096)
    };
    let unity = render(0.0);
    let limited = render(12.0);
    let mut left_difference = 0.0_f32;
    for (unity, limited) in unity.chunks_exact(2).zip(limited.chunks_exact(2)) {
        assert_eq!(unity[1], limited[1]);
        left_difference = left_difference.max((unity[0] - limited[0]).abs());
    }
    assert!(left_difference > 0.05);
}

#[test]
fn replacing_source_clears_old_limiter_reduction_and_preserves_gain_setting() {
    let engine = single_deck(12.0, 0.0, 1.0);
    let fresh = test_engine(48_000);
    for engine in [&engine, &fresh] {
        let mut buffer = sine_buffer(48_000, 440.0, 1.0);
        for sample in &mut Arc::get_mut(&mut buffer).unwrap().samples {
            *sample *= 0.05;
        }
        let wave = Arc::new(crate::compute_preview_waveform(&buffer, 256));
        engine
            .load_buffer_if(
                DeckId::A,
                TrackId(3),
                buffer,
                wave,
                String::new(),
                String::new(),
                || true,
            )
            .unwrap();
        engine
            .dispatch(Command::SetCrossfader { value: -1.0 })
            .unwrap();
        engine.dispatch(Command::SetMaster { value: 1.0 }).unwrap();
    }
    assert_eq!(engine.snapshot().decks[0].limiter_gain_db, 12.0);
    fresh
        .dispatch(Command::SetDeckLimiterGain {
            deck: DeckId::A,
            db: 12.0,
        })
        .unwrap();
    for engine in [&engine, &fresh] {
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
    }
    // Render enough to settle routing, but less than the limiter's 80 ms release.
    let replaced = engine.render_offline(1024);
    let fresh = fresh.render_offline(1024);
    assert!(energy(&fresh[1024..]) > 0.001);
    for (replaced, fresh) in replaced[1024..].iter().zip(&fresh[1024..]) {
        assert!((replaced - fresh).abs() < 1e-5);
    }
}
