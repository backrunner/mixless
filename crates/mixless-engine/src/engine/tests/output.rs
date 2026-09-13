use super::*;

#[test]
fn source_normalization_reaches_both_master_and_cue_without_moving_manual_gain() {
    let render = |amplitude: f32| {
        let engine = test_engine(48_000);
        let mut buffer = sine_buffer(48_000, 1000., 3.);
        let audio = Arc::get_mut(&mut buffer).unwrap();
        for x in &mut audio.samples {
            *x *= amplitude;
        }
        audio.loudness = crate::Loudness::measure(&audio.samples, audio.sample_rate);
        let wave = Arc::new(crate::compute_preview_waveform(&buffer, 256));
        engine
            .load_buffer_if(
                DeckId::A,
                TrackId(1),
                buffer,
                wave,
                String::new(),
                String::new(),
                || true,
            )
            .unwrap();
        engine
            .dispatch(Command::SetCrossfader { value: -1. })
            .unwrap();
        engine
            .dispatch(Command::SetPfl {
                deck: DeckId::A,
                on: true,
            })
            .unwrap();
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        engine.render_offline(48_000);
        let (producer, mut consumer) = rtrb::RingBuffer::new(4096);
        let mut rt = engine.rt.lock().unwrap();
        rt.cue_output = Some(device::CueWriter::new(producer, 48_000, 48_000));
        let mut output = vec![0.; 4096 * 2];
        engine.shared.process_block(&mut rt, &mut output, 2);
        let cue: Vec<_> = std::iter::from_fn(|| consumer.pop().ok())
            .flatten()
            .collect();
        assert_eq!(engine.snapshot().decks[0].gain_db, 0.);
        [energy(&output[4000..]), energy(&cue[4000..])]
    };
    let quiet = render(0.2);
    let loud = render(1.6);
    for (quiet, loud) in quiet.into_iter().zip(loud) {
        assert!(quiet > 0.0001);
        assert!((10. * (quiet / loud).log10()).abs() < 0.1);
    }
}

#[test]
fn headphone_bus_is_pre_fader_and_independent_of_master() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine
        .dispatch(Command::SetChannelFader {
            deck: DeckId::A,
            value: 0.0,
        })
        .unwrap();
    engine.dispatch(Command::SetMaster { value: 0.0 }).unwrap();
    engine
        .dispatch(Command::SetPfl {
            deck: DeckId::A,
            on: true,
        })
        .unwrap();
    let (producer, mut consumer) = rtrb::RingBuffer::new(8192);
    let mut rt = engine.rt.lock().unwrap();
    rt.cue_output = Some(device::CueWriter::new(producer, 48_000, 48_000));
    let mut output = vec![0.0; 4096 * 2];
    engine.shared.process_block(&mut rt, &mut output, 2);
    assert!(energy(&output[4000..]) < 1e-8);
    let headphones: Vec<_> = std::iter::from_fn(|| consumer.pop().ok())
        .flatten()
        .collect();
    assert!(energy(&headphones[4000..]) > 0.001);
    engine
        .dispatch(Command::SetPfl {
            deck: DeckId::A,
            on: false,
        })
        .unwrap();
    engine.shared.process_block(&mut rt, &mut output, 2);
    let headphones: Vec<_> = std::iter::from_fn(|| consumer.pop().ok())
        .flatten()
        .collect();
    assert!(energy(&headphones[4000..]) < 1e-8);
    assert!(!engine.snapshot().pfl_available);
}

#[test]
#[ignore = "opens real CoreAudio output devices"]
fn audio_device_reconfiguration_smoke() {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    assert!(engine.audio_available());
    let master = engine.snapshot().device_name;
    let mut config = device::AudioConfig {
        buffer_frames: Some(256),
        ..Default::default()
    };
    engine.configure_audio(config.clone()).unwrap();
    let other = device::output_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.name != master)
        .unwrap();
    config.cue_device = Some(other.name);
    engine.configure_audio(config.clone()).unwrap();
    assert!(engine.snapshot().pfl_available);
    let mut invalid = config.clone();
    invalid.master_device = Some("mixless-missing-device-test".into());
    assert!(engine.configure_audio(invalid).is_err());
    assert_eq!(engine.audio_config(), config);
    assert!(engine.audio_available());
    invalid = config.clone();
    invalid.cue_device = Some(master);
    assert!(engine.configure_audio(invalid).is_err());
    assert_eq!(engine.audio_config(), config);
    engine
        .configure_audio(device::AudioConfig::default())
        .unwrap();
    assert!(!engine.snapshot().pfl_available);
}

#[test]
fn source_lock_contention_does_not_interrupt_playback() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine.render_offline(2048);
    let start = engine.snapshot().decks[0].frame;
    let _guard = engine.shared.decks[0].buffer.lock().unwrap();
    let output = engine.render_offline(256);
    assert!(energy(&output) > 0.001);
    assert_eq!(engine.snapshot().decks[0].frame, start + 256);
    assert_eq!(engine.snapshot().xrun_count, 0);
}

#[test]
fn offline_silence_is_finite_zero() {
    let eng = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48_000,
        block_frames: 256,
    })
    .unwrap();
    let out = eng.render_offline(512);
    assert_eq!(out.len(), 1024);
    assert!(out.iter().all(|s| s.is_finite() && *s == 0.0));
}

#[test]
fn two_decks_mix_without_nan() {
    let eng = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48_000,
        block_frames: 256,
    })
    .unwrap();
    {
        let slot = &eng.shared.decks[0];
        *slot.buffer.lock().unwrap() = Some(sine_buffer(48_000, 220.0, 1.0));
        slot.frames.store(48_000, Ordering::Relaxed);
        slot.src_sr.store(48_000, Ordering::Relaxed);
        slot.playing.store(true, Ordering::Relaxed);
    }
    {
        let slot = &eng.shared.decks[1];
        *slot.buffer.lock().unwrap() = Some(sine_buffer(48_000, 330.0, 1.0));
        slot.frames.store(48_000, Ordering::Relaxed);
        slot.src_sr.store(48_000, Ordering::Relaxed);
        slot.playing.store(true, Ordering::Relaxed);
    }
    eng.dispatch(Command::SetCrossfader { value: 0.0 }).unwrap();
    let out = eng.render_offline(1024);
    assert!(out.iter().all(|s| s.is_finite()));
    let energy: f32 = out.iter().map(|s| s * s).sum();
    assert!(energy > 0.01, "expected audible energy, got {energy}");
}

#[test]
fn dispatch_updates_snapshot_without_audio_thread() {
    let eng = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48_000,
        block_frames: 256,
    })
    .unwrap();
    eng.dispatch(Command::SetCrossfader { value: -1.0 })
        .unwrap();
    eng.dispatch(Command::SetMaster { value: 0.25 }).unwrap();
    let snap = eng.snapshot();
    assert!((snap.xfader - (-1.0)).abs() < 0.01);
    assert!((snap.master - 0.25).abs() < 0.01);
}
