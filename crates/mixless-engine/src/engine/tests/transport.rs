use super::*;

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
        for beats in [0.0625,0.125,0.25,0.5,1.,2.,4.] {
            engine.dispatch(Command::SetLoopBeats { deck: DeckId::A, beats, on: true }).unwrap();
            let snapshot = engine.snapshot();
            let d = &snapshot.decks[0];
            assert_eq!(d.loop_beats, beats);
            assert!((d.loop_start_frame as f64 - source_rate as f64 * 0.3).abs() <= 1.);
            let expected = source_rate as f32 * 0.5 * beats;
            assert!(((d.loop_end_frame - d.loop_start_frame) as f32 - expected).abs() <= 1.);
        }
        engine.dispatch(Command::SetLoopBeats { deck: DeckId::A, beats: 0.0625, on: true }).unwrap();
        engine.dispatch(Command::PlayPause { deck: DeckId::A }).unwrap();
        for _ in 0..100 { engine.render_offline(128); }
        let d = engine.snapshot().decks[0].clone();
        assert!(d.frame >= d.loop_start_frame && d.frame <= d.loop_end_frame + 1);
        engine.dispatch(Command::LoopDouble { deck: DeckId::A }).unwrap();
        assert_eq!(engine.snapshot().decks[0].loop_beats,0.125);
        engine.dispatch(Command::LoopHalve { deck: DeckId::A }).unwrap();
        assert_eq!(engine.snapshot().decks[0].loop_beats,0.0625);
    }
}

#[test]
fn quantized_fractional_loop_uses_measured_grid_origin() {
    let engine = test_engine(48_000);
    let slot = &engine.shared.decks[0];
    let id = TrackId(slot.track_id.load(Ordering::Relaxed) as i64);
    engine.set_beat_grid(DeckId::A,id,mixless_protocol::TempoMap {
        global_bpm: 120., meter_num: 4, meter_den: 4,
        beats: (0..16).map(|i| 0.1 + i as f32 * 0.5).collect(),
        downbeats: vec![0.1,2.1], ..Default::default()
    }).unwrap();
    slot.set_playhead(0.365 * 48_000.);
    engine.dispatch(Command::SetLoopBeats { deck: DeckId::A, beats: 0.5, on: true }).unwrap();
    let d = engine.snapshot().decks[0].clone();
    assert!(d.loop_start_frame.abs_diff(16_800) <= 1);
    assert_eq!(d.loop_end_frame - d.loop_start_frame,12_000);
}
