use super::*;

#[test]
fn deck_key_and_tempo_are_independent_end_to_end() {
    for (rate, key, keylock) in [
        (0.88, 0.0, true),
        (1.12, 0.0, true),
        (1.12, 12.0, true),
        (1.0, -5.0, true),
        (1.12, 0.0, false),
        (1.12, 7.0, false),
    ] {
        let engine = test_engine(48_000);
        engine.set_bpm(DeckId::A, 120.0);
        engine
            .dispatch(Command::SetRate {
                deck: DeckId::A,
                rate,
            })
            .unwrap();
        engine
            .dispatch(Command::SetPitchSemitones {
                deck: DeckId::A,
                semitones: key,
            })
            .unwrap();
        engine
            .dispatch(Command::SetKeyLock {
                deck: DeckId::A,
                on: keylock,
            })
            .unwrap();
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        left_channel(&engine, 12_800);
        let start = engine.snapshot().decks[0].frame;
        let samples = left_channel(&engine, 48_000);
        let measured = crate::stretch::frequency(&samples, 48000.0);
        let expected =
            440.0 * 2.0f64.powf(key as f64 / 12.0) * if keylock { 1.0 } else { rate as f64 };
        assert!(
            (measured - expected).abs() < expected * 0.0015,
            "rate={rate} key={key} lock={keylock}: {measured} vs {expected}"
        );
        let snapshot = &engine.snapshot().decks[0];
        assert!((snapshot.sounding_bpm - 120.0 * rate).abs() < 0.01);
        assert!((snapshot.frame as f64 - start as f64 - rate as f64 * 48000.0).abs() < 2.0);
    }
}

#[test]
fn sync_uses_tempo_and_preserves_key_offset() {
    let engine = test_engine(48_000);
    engine.set_bpm(DeckId::A, 100.0);
    engine.set_bpm(DeckId::B, 120.0);
    for (deck, period) in [(DeckId::A, 0.6), (DeckId::B, 0.5)] {
        engine
            .set_beat_grid(
                deck,
                TrackId(0),
                mixless_protocol::TempoMap {
                    beats: (0..8).map(|i| i as f32 * period).collect(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    engine
        .dispatch(Command::SetRate {
            deck: DeckId::B,
            rate: 1.1,
        })
        .unwrap();
    engine
        .dispatch(Command::SetPitchSemitones {
            deck: DeckId::A,
            semitones: 7.0,
        })
        .unwrap();
    engine
        .dispatch(Command::Sync {
            deck: DeckId::A,
            keylock: true,
        })
        .unwrap();
    let snapshot = engine.snapshot();
    assert!((snapshot.decks[0].rate - 1.32).abs() < 0.002);
    assert!((snapshot.decks[0].sounding_bpm - snapshot.decks[1].sounding_bpm).abs() < 0.2);
    assert_eq!(snapshot.decks[0].pitch_semitones, 7.0);
    assert!(snapshot.decks[0].keylock);
    engine
        .dispatch(Command::Sync {
            deck: DeckId::A,
            keylock: false,
        })
        .unwrap();
    assert!(!engine.snapshot().decks[0].keylock);
}

#[test]
fn music_scratch_cue_and_pause_transitions_preserve_transport() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::SetRate {
            deck: DeckId::A,
            rate: 1.12,
        })
        .unwrap();
    engine
        .dispatch(Command::SetPitchSemitones {
            deck: DeckId::A,
            semitones: 12.0,
        })
        .unwrap();
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    left_channel(&engine, 12_800);
    let held = engine.snapshot().decks[0].frame;
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    left_channel(&engine, 2048);
    assert_eq!(engine.snapshot().decks[0].frame, held);
    assert!(energy(&engine.render_offline(512)) < 1e-10);
    engine
        .dispatch(Command::Jog {
            deck: DeckId::A,
            delta_frames: 3000.0,
        })
        .unwrap();
    assert!(energy(&engine.render_offline(1024)) > 0.001);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: false,
        })
        .unwrap();
    left_channel(&engine, 4800);
    let samples = left_channel(&engine, 12_800);
    assert!((crate::stretch::frequency(&samples, 48000.0) - 880.0).abs() < 8.0);
    engine
        .dispatch(Command::SetCue {
            deck: DeckId::A,
            index: 0,
            frame: 96123,
        })
        .unwrap();
    engine
        .dispatch(Command::JumpCue {
            deck: DeckId::A,
            index: 0,
        })
        .unwrap();
    let before = *samples.last().unwrap();
    let after = left_channel(&engine, 1024);
    let mut worst = (after[0] - before).abs();
    for pair in after.windows(2) {
        worst = worst.max((pair[1] - pair[0]).abs());
    }
    assert!(worst < 0.10, "music cue discontinuity {worst}");
    assert!((engine.snapshot().decks[0].frame as f64 - 96123.0 - 1024.0 * 1.12).abs() < 2.0);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    left_channel(&engine, 1024);
    let paused = engine.snapshot().decks[0].frame;
    assert!(energy(&engine.render_offline(1024)) < 1e-9);
    assert_eq!(engine.snapshot().decks[0].frame, paused);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    left_channel(&engine, 4800);
    let samples = left_channel(&engine, 12800);
    assert!((crate::stretch::frequency(&samples, 48000.0) - 880.0).abs() < 8.0);
}

#[test]
fn music_loop_and_direction_changes_are_crossfaded() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::SetRate {
            deck: DeckId::A,
            rate: 1.12,
        })
        .unwrap();
    engine
        .dispatch(Command::SetPitchSemitones {
            deck: DeckId::A,
            semitones: 12.0,
        })
        .unwrap();
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    let initial = left_channel(&engine, 12_800);
    let mut previous = *initial.last().unwrap();
    for command in [
        Command::SetLoop {
            deck: DeckId::A,
            bars: 1,
            on: true,
        },
        Command::SetLoop {
            deck: DeckId::A,
            bars: 2,
            on: true,
        },
        Command::SetLoop {
            deck: DeckId::A,
            bars: 2,
            on: false,
        },
        Command::SetReverse {
            deck: DeckId::A,
            on: true,
        },
        Command::SetReverse {
            deck: DeckId::A,
            on: false,
        },
    ] {
        engine.dispatch(command).unwrap();
        let samples = left_channel(&engine, 1024);
        let mut worst = 0.0f32;
        for sample in samples {
            assert!(sample.is_finite());
            worst = worst.max((sample - previous).abs());
            previous = sample;
        }
        assert!(worst < 0.10, "music loop/direction discontinuity {worst}");
    }
}

#[test]
fn loop_and_reverse_keep_independent_key() {
    for reverse in [false, true] {
        let engine = test_engine(48_000);
        engine.shared.decks[0].set_playhead(if reverse { 150000.0 } else { 0.0 });
        engine.set_bpm(DeckId::A, 600.0);
        engine
            .dispatch(Command::SetRate {
                deck: DeckId::A,
                rate: 0.88,
            })
            .unwrap();
        engine
            .dispatch(Command::SetPitchSemitones {
                deck: DeckId::A,
                semitones: 7.0,
            })
            .unwrap();
        engine
            .dispatch(Command::SetReverse {
                deck: DeckId::A,
                on: reverse,
            })
            .unwrap();
        if !reverse {
            engine
                .dispatch(Command::SetLoop {
                    deck: DeckId::A,
                    bars: 1,
                    on: true,
                })
                .unwrap();
        }
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        left_channel(&engine, 4800);
        let samples = left_channel(&engine, 48000);
        let expected = 440.0 * 2.0f64.powf(7.0 / 12.0);
        let measured = crate::stretch::frequency(&samples, 48000.0);
        assert!(
            (measured - expected).abs() < expected * 0.015,
            "reverse={reverse}: {measured}"
        );
        if !reverse {
            assert!(engine.snapshot().decks[0].frame < 19200);
        }
    }
}
