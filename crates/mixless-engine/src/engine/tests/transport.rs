use super::*;

#[test]
fn eject_clears_track_state_and_keeps_the_other_deck_playing() {
    for deck in [DeckId::A, DeckId::B] {
        let engine = test_engine(48_000);
        for loaded in [DeckId::A, DeckId::B] {
            let buffer = sine_buffer(48_000, 440., 4.);
            let wave = Arc::new(crate::waveform::compute_waveform(&buffer, 128));
            engine
                .load_buffer_if(
                    loaded,
                    TrackId(loaded.index() as i64 + 1),
                    buffer,
                    wave,
                    "Title".into(),
                    "Artist".into(),
                    || true,
                )
                .unwrap();
            engine.set_bpm(loaded, 120.);
            engine
                .dispatch(Command::PlayPause { deck: loaded })
                .unwrap();
        }
        engine.set_cue_frame(deck, 0, 1_000);
        engine
            .dispatch(Command::SetLoopBeats {
                deck,
                beats: 1.,
                on: true,
            })
            .unwrap();
        engine.render_offline(512);
        let other = 1 - deck.index();
        let before = engine.snapshot().decks[other].clone();

        engine.dispatch(Command::Eject { deck }).unwrap();
        let snapshot = engine.snapshot();
        let empty = snapshot.deck(deck);
        assert!(empty.track_id.is_none() && empty.title.is_none() && empty.artist.is_none());
        assert!(!empty.playing && !empty.loop_on && !empty.roll && !empty.brake);
        assert_eq!(
            (empty.frame, empty.frames, empty.src_sample_rate),
            (0, 0, 0)
        );
        assert_eq!(empty.sounding_bpm, 0.);
        assert_eq!((empty.loop_start_frame, empty.loop_end_frame), (0, 0));
        assert!(empty.cues.iter().all(Option::is_none));
        assert!(empty.temporary_cue_frame.is_none());
        assert!(engine.deck_waveform(deck).is_none());
        assert_eq!(snapshot.decks[other], before);
        assert!(energy(&engine.render_offline(512)) > 0.001);
        let after = engine.snapshot();
        assert!(after.decks[other].playing && after.decks[other].frame > before.frame);
        assert_eq!(after.deck(deck).frame, 0);
        // Unloading an already empty deck is harmless.
        engine.dispatch(Command::Eject { deck }).unwrap();
        assert_eq!(engine.snapshot().deck(deck).frames, 0);
    }
}

#[test]
fn eject_has_a_bounded_declick_tail_and_preview_never_leaks_to_master() {
    for sample_rate in [44_100, 48_000] {
        for preview in [false, true] {
            let engine = test_engine(sample_rate);
            if preview {
                engine
                    .dispatch(Command::BeginCuePreview {
                        deck: DeckId::A,
                        frame: 0,
                    })
                    .unwrap();
            } else {
                engine
                    .dispatch(Command::PlayPause { deck: DeckId::A })
                    .unwrap();
            }
            let before = engine.render_offline(1000);
            let last = before[before.len() - 2];
            engine.eject(DeckId::A);
            let after = engine.render_offline(sample_rate as usize / 100);
            assert!((after[0] - last).abs() < 0.02, "eject discontinuity");
            assert!(after.iter().all(|s| s.is_finite()));
            if preview {
                assert!(
                    after.iter().all(|s| s.abs() < 1e-8),
                    "monitor leaked to master"
                );
            }
            assert!(after[after.len() / 2..].iter().all(|s| s.abs() < 1e-8));
            assert!(!engine.snapshot().decks[0].cue_previewing);
            assert!(engine.render_offline(512).iter().all(|s| s.abs() < 1e-8));
        }
    }
}

#[test]
fn immediate_reload_of_the_same_cached_pcm_starts_from_zero_without_old_transport() {
    let engine = test_engine(48_000);
    let buffer = sine_buffer(48_000, 440., 4.);
    let wave = Arc::new(crate::waveform::compute_waveform(&buffer, 128));
    let load = || {
        engine
            .load_buffer_if(
                DeckId::A,
                TrackId(1),
                buffer.clone(),
                wave.clone(),
                "Same track".into(),
                "Artist".into(),
                || true,
            )
            .unwrap()
    };
    load();
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine.render_offline(20_000);
    engine
        .dispatch(Command::SetBrake {
            deck: DeckId::A,
            on: true,
        })
        .unwrap();
    engine.render_offline(1000);
    engine.eject(DeckId::A);
    load(); // No callback runs between eject and loading the exact same Arc.
    engine.render_offline(256);
    let deck = &engine.snapshot().decks[0];
    assert!(!deck.playing && !deck.brake && !deck.loop_on);
    assert_eq!(deck.frame, 0);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine.render_offline(256);
    assert!(engine.snapshot().decks[0].frame < 512);
}

#[test]
fn roll_loops_the_selected_fraction_and_brake_ramps_to_stop() {
    let engine = test_engine(48_000);
    engine.set_bpm(DeckId::A, 120.0);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine
        .dispatch(Command::SetRoll {
            deck: DeckId::A,
            division: 1,
            on: true,
        })
        .unwrap();
    left_channel(&engine, 48_000);
    assert!(engine.snapshot().decks[0].roll);
    engine
        .dispatch(Command::SetRoll {
            deck: DeckId::A,
            division: 1,
            on: false,
        })
        .unwrap();
    engine
        .dispatch(Command::SetBrake {
            deck: DeckId::A,
            on: true,
        })
        .unwrap();
    left_channel(&engine, 4_800);
    assert!(engine.snapshot().decks[0].brake);
    assert!(engine.snapshot().decks[0].playing);
}

#[test]
fn scratch_paused_deck_forward_backward_hold_and_release() {
    let engine = test_engine(48_000);
    let slot = &engine.shared.decks[0];
    slot.set_playhead(48_000.0);
    engine.render_offline(128);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    let start = engine.snapshot().decks[0].frame;
    engine
        .dispatch(Command::Jog {
            deck: DeckId::A,
            delta_frames: 2400.0,
        })
        .unwrap();
    let forward = engine.render_offline(1024);
    assert!(energy(&forward) > 0.001);
    let ahead = engine.snapshot().decks[0].frame;
    assert!(ahead > start + 2000 && ahead <= start + 2400);
    engine
        .dispatch(Command::Jog {
            deck: DeckId::A,
            delta_frames: -4800.0,
        })
        .unwrap();
    let backward = engine.render_offline(2048);
    assert!(energy(&backward) > 0.001);
    assert!(engine.snapshot().decks[0].frame < start - 2000);
    engine.render_offline(4800);
    assert!(energy(&engine.render_offline(512)) < 1e-10);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: false,
        })
        .unwrap();
    engine.render_offline(512);
    assert!(!engine.snapshot().decks[0].playing);
    assert!(energy(&engine.render_offline(512)) < 1e-10);
}

#[test]
fn scratch_touch_holds_playing_deck_and_release_resumes() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine.render_offline(2048);
    let start = engine.snapshot().decks[0].frame;
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    engine.render_offline(2048);
    assert_eq!(engine.snapshot().decks[0].frame, start);
    assert!(energy(&engine.render_offline(512)) < 1e-10);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: false,
        })
        .unwrap();
    let resumed = engine.render_offline(1024);
    assert!(engine.snapshot().decks[0].frame > start + 800);
    assert!(energy(&resumed) > 0.001);
    assert!(engine.snapshot().decks[0].playing);
}

#[test]
fn slip_returns_to_background_timeline() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine
        .dispatch(Command::SetVinylMode {
            deck: DeckId::A,
            vinyl: true,
            slip: true,
        })
        .unwrap();
    engine.render_offline(2000);
    let start = engine.snapshot().decks[0].frame;
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    engine
        .dispatch(Command::Jog {
            deck: DeckId::A,
            delta_frames: 4000.0,
        })
        .unwrap();
    engine.render_offline(1000);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: false,
        })
        .unwrap();
    engine.render_offline(256);
    let resumed = engine.snapshot().decks[0].frame;
    assert!(
        (resumed as i64 - (start + 1256) as i64).abs() < 150,
        "slip: {start} -> {resumed}"
    );
}

#[test]
fn cue_crossfade_has_bounded_sample_discontinuity() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    let before = engine.render_offline(1000);
    engine
        .dispatch(Command::SetCue {
            deck: DeckId::A,
            index: 0,
            frame: 24513,
        })
        .unwrap();
    engine
        .dispatch(Command::JumpCue {
            deck: DeckId::A,
            index: 0,
        })
        .unwrap();
    let after = engine.render_offline(512);
    let mut previous = before[before.len() - 2];
    let mut worst = 0.0f32;
    for frame in after.chunks_exact(2) {
        worst = worst.max((frame[0] - previous).abs());
        previous = frame[0];
    }
    assert!(worst < 0.05, "cue discontinuity {worst}");
}

#[test]
fn cue_jump_is_click_free_offline() {
    let eng = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48_000,
        block_frames: 256,
    })
    .unwrap();
    {
        let slot = &eng.shared.decks[0];
        *slot.buffer.lock().unwrap() = Some(sine_buffer(48_000, 220.0, 2.0));
        slot.frames.store(96_000, Ordering::Relaxed);
        slot.src_sr.store(48_000, Ordering::Relaxed);
        slot.playing.store(true, Ordering::Relaxed);
    }
    eng.dispatch(Command::SetCue {
        deck: DeckId::A,
        index: 0,
        frame: 48_000,
    })
    .unwrap();
    let _ = eng.render_offline(128);
    eng.dispatch(Command::JumpCue {
        deck: DeckId::A,
        index: 0,
    })
    .unwrap();
    let out = eng.render_offline(512);
    assert!(out.iter().all(|s| s.is_finite()));
    assert_eq!(eng.snapshot().decks[0].src_sample_rate, 48_000);
}

#[test]
fn fractional_loops_use_beats_and_keep_the_anchor_when_resized() {
    for source_rate in [44_100, 48_000] {
        let engine = test_engine(source_rate);
        engine.set_bpm(DeckId::A, 120.);
        engine.dispatch(Command::SetQuantize { on: false }).unwrap();
        let slot = &engine.shared.decks[0];
        slot.set_playhead(source_rate as f64 * 0.3);
        for beats in [0.0625, 0.125, 0.25, 0.5, 1., 2., 4.] {
            engine
                .dispatch(Command::SetLoopBeats {
                    deck: DeckId::A,
                    beats,
                    on: true,
                })
                .unwrap();
            let snapshot = engine.snapshot();
            let d = &snapshot.decks[0];
            assert_eq!(d.loop_beats, beats);
            assert!((d.loop_start_frame as f64 - source_rate as f64 * 0.3).abs() <= 1.);
            let expected = source_rate as f32 * 0.5 * beats;
            assert!(((d.loop_end_frame - d.loop_start_frame) as f32 - expected).abs() <= 1.);
        }
        engine
            .dispatch(Command::SetLoopBeats {
                deck: DeckId::A,
                beats: 0.0625,
                on: true,
            })
            .unwrap();
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        for _ in 0..100 {
            engine.render_offline(128);
        }
        let d = engine.snapshot().decks[0].clone();
        assert!(d.frame >= d.loop_start_frame && d.frame <= d.loop_end_frame + 1);
        engine
            .dispatch(Command::LoopDouble { deck: DeckId::A })
            .unwrap();
        assert_eq!(engine.snapshot().decks[0].loop_beats, 0.125);
        engine
            .dispatch(Command::LoopHalve { deck: DeckId::A })
            .unwrap();
        assert_eq!(engine.snapshot().decks[0].loop_beats, 0.0625);
    }
}

#[test]
fn quantized_fractional_loop_uses_measured_grid_origin() {
    let engine = test_engine(48_000);
    let slot = &engine.shared.decks[0];
    let id = TrackId(slot.track_id.load(Ordering::Relaxed) as i64);
    engine
        .set_beat_grid(
            DeckId::A,
            id,
            mixless_protocol::TempoMap {
                global_bpm: 120.,
                meter_num: 4,
                meter_den: 4,
                beats: (0..16).map(|i| 0.1 + i as f32 * 0.5).collect(),
                downbeats: vec![0.1, 2.1],
                ..Default::default()
            },
        )
        .unwrap();
    slot.set_playhead(0.365 * 48_000.);
    engine
        .dispatch(Command::SetLoopBeats {
            deck: DeckId::A,
            beats: 0.5,
            on: true,
        })
        .unwrap();
    let d = engine.snapshot().decks[0].clone();
    assert!(d.loop_start_frame.abs_diff(16_800) <= 1);
    assert_eq!(d.loop_end_frame - d.loop_start_frame, 12_000);
}

#[test]
fn quantized_loop_never_anchors_ahead_of_the_playhead() {
    let engine = test_engine(48_000);
    let slot = &engine.shared.decks[0];
    let id = TrackId(slot.track_id.load(Ordering::Relaxed) as i64);
    engine
        .set_beat_grid(
            DeckId::A,
            id,
            mixless_protocol::TempoMap {
                global_bpm: 120.,
                meter_num: 4,
                meter_den: 4,
                beats: (0..16).map(|i| 0.1 + i as f32 * 0.5).collect(),
                downbeats: vec![0.1, 2.1],
                ..Default::default()
            },
        )
        .unwrap();
    for beats in [0.0625, 0.125, 0.25, 0.5, 1., 4.] {
        engine
            .dispatch(Command::SetLoopBeats {
                deck: DeckId::A,
                beats,
                on: false,
            })
            .unwrap();
        slot.set_playhead(0.59 * 48_000.);
        engine
            .dispatch(Command::SetLoopBeats {
                deck: DeckId::A,
                beats,
                on: true,
            })
            .unwrap();
        let d = engine.snapshot().decks[0].clone();
        assert!(
            d.loop_start_frame <= d.frame,
            "{beats}: {} > {}",
            d.loop_start_frame,
            d.frame
        );
        assert!(d.loop_end_frame > d.frame);
    }
}
