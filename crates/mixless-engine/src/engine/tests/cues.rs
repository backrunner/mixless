use super::*;

#[test]
fn temporary_cue_is_independent_and_never_stops_a_playing_deck() {
    let engine = test_engine(48_000);
    let deck = DeckId::A;
    engine.set_cue_frame(deck, 0, 24000);
    engine.dispatch(Command::PlayPause { deck }).unwrap();
    engine.render_offline(12000);
    engine
        .dispatch(Command::TriggerTemporaryCue { deck })
        .unwrap();
    let frame = engine.snapshot().deck(deck).temporary_cue_frame.unwrap();
    assert!((11999..=12001).contains(&frame));
    engine.render_offline(12000);
    engine
        .dispatch(Command::TriggerTemporaryCue { deck })
        .unwrap();
    engine.render_offline(128);
    let snapshot = engine.snapshot();
    assert!(snapshot.deck(deck).playing);
    assert!((frame + 127..=frame + 129).contains(&snapshot.deck(deck).frame));
    assert_eq!(snapshot.deck(deck).cues[0], Some(24000));
    engine
        .dispatch(Command::ClearTemporaryCue { deck })
        .unwrap();
    assert_eq!(engine.snapshot().deck(deck).temporary_cue_frame, None);
    engine.dispatch(Command::PlayPause { deck }).unwrap();
    engine
        .dispatch(Command::JumpCue { deck, index: 0 })
        .unwrap();
    engine.render_offline(512);
    assert!(!engine.snapshot().deck(deck).playing);
    assert_eq!(engine.snapshot().deck(deck).frame, 24000);
}

#[test]
fn repeated_cues_crossfade_without_silence_and_land_in_the_next_block() {
    for sr in [44100, 48000] {
        let engine = test_engine(sr);
        let deck = DeckId::A;
        engine
            .dispatch(Command::SetCrossfader { value: -1. })
            .unwrap();
        engine
            .dispatch(Command::SetRate { deck, rate: 1.08 })
            .unwrap();
        engine
            .dispatch(Command::SetPitchSemitones {
                deck,
                semitones: 2.,
            })
            .unwrap();
        engine.dispatch(Command::PlayPause { deck }).unwrap();
        for i in 0..8 {
            engine.set_cue_frame(deck, i, sr as u64 * (i as u64 + 1) / 3);
        }
        engine.render_offline(4096);
        let mut output = Vec::new();
        // 64 frames is shorter than the de-click tail: exercise retriggers
        // while a previous transition is still being rendered.
        for i in 0..128 {
            engine
                .dispatch(Command::JumpCue {
                    deck,
                    index: (i % 8) as u8,
                })
                .unwrap();
            let block = engine.render_offline(64);
            assert!(energy(&block) > 0.00005, "silent cue block {i} at {sr}");
            assert!(block.iter().all(|x| x.is_finite() && x.abs() < 1.));
            let snapshot = engine.snapshot();
            let target = snapshot.deck(deck).cues[i % 8].unwrap();
            assert!((target + 60..=target + 80).contains(&snapshot.deck(deck).frame));
            assert!(snapshot.deck(deck).playing);
            output.extend(block.chunks_exact(2).map(|s| s[0]));
        }
        let max_step = output
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0f32, f32::max);
        assert!(max_step < 0.18, "discontinuous retrigger: {max_step}");
    }
}

#[test]
fn brake_decelerates_with_key_lock_then_stops_and_restarts_at_original_tempo() {
    let engine = test_engine(48_000);
    let deck = DeckId::A;
    engine
        .dispatch(Command::SetRate { deck, rate: 1.08 })
        .unwrap();
    engine
        .dispatch(Command::SetPitchSemitones {
            deck,
            semitones: 2.,
        })
        .unwrap();
    engine.dispatch(Command::PlayPause { deck }).unwrap();
    engine.render_offline(4096);
    engine
        .dispatch(Command::SetBrake { deck, on: true })
        .unwrap();
    let mut previous = engine.snapshot().deck(deck).frame;
    let mut advances = vec![];
    for _ in 0..6 {
        let block = engine.render_offline(9600);
        assert!(block.iter().all(|s| s.is_finite()));
        let frame = engine.snapshot().deck(deck).frame;
        advances.push(frame - previous);
        previous = frame;
    }
    assert!(advances.windows(2).all(|w| w[1] < w[0]), "{advances:?}");
    engine.render_offline(512);
    let d = engine.snapshot().deck(deck).clone();
    assert!(!d.playing && !d.brake);
    assert!(d.keylock);
    assert_eq!(d.rate, 1.08);
    assert_eq!(d.pitch_semitones, 2.);
    assert!(energy(&engine.render_offline(1024)) < 1e-8);
    engine.dispatch(Command::PlayPause { deck }).unwrap();
    assert!(energy(&engine.render_offline(2048)) > 0.001);
    assert!(engine.snapshot().deck(deck).frame > d.frame + 2000);
}

#[test]
fn empty_transport_is_silent_and_reloading_clears_temporary_state() {
    let engine = Engine::new(EngineConfig {
        offline: true,
        ..Default::default()
    })
    .unwrap();
    let deck = DeckId::A;
    engine.dispatch(Command::PlayPause { deck }).unwrap();
    engine
        .dispatch(Command::TriggerTemporaryCue { deck })
        .unwrap();
    assert!(!engine.snapshot().deck(deck).playing);
    assert_eq!(engine.snapshot().deck(deck).temporary_cue_frame, None);
    let buffer = sine_buffer(48000, 220., 1.);
    let wave = Arc::new(crate::waveform::compute_waveform(&buffer, 512));
    engine
        .load_buffer_if(
            deck,
            TrackId(1),
            buffer.clone(),
            wave.clone(),
            "one".into(),
            "".into(),
            || true,
        )
        .unwrap();
    engine
        .dispatch(Command::TriggerTemporaryCue { deck })
        .unwrap();
    assert_eq!(engine.snapshot().deck(deck).temporary_cue_frame, Some(0));
    engine
        .dispatch(Command::SetCueKind {
            track_id: TrackId(1),
            index: 0,
            kind: CueKind::Out,
        })
        .unwrap();
    engine
        .load_buffer_if(
            deck,
            TrackId(2),
            buffer,
            wave,
            "two".into(),
            "".into(),
            || true,
        )
        .unwrap();
    let d = engine.snapshot().deck(deck).clone();
    assert_eq!(d.temporary_cue_frame, None);
    assert_eq!(d.cue_kinds[0], CueKind::Hot);
}
