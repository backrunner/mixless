use super::*;

#[test]
fn recording_matches_post_master_output_and_seals_wav() {
    let engine = test_engine(48_000);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mix.wav");
    engine.dispatch(Command::SetMaster { value: 0.37 }).unwrap();
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine
        .dispatch(Command::PlayPause { deck: DeckId::B })
        .unwrap();
    engine.start_recording(&path).unwrap();
    assert!(engine
        .start_recording(&directory.path().join("duplicate.wav"))
        .is_err());
    let mut actual = Vec::new();
    for _ in 0..16 {
        actual.extend(engine.render_offline(256));
    }
    let status = engine.stop_recording().unwrap();
    assert!(!status.active);
    assert_eq!(status.dropped_frames, 0);
    assert!((status.seconds - 4096. / 48_000.).abs() < 1e-9);
    let mut wav = hound::WavReader::open(&path).unwrap();
    assert_eq!(wav.spec().sample_rate, 48_000);
    assert_eq!(wav.spec().channels, 2);
    let recorded: Vec<f32> = wav.samples().map(Result::unwrap).collect();
    assert_eq!(recorded, actual);
    assert!(energy(&recorded) > 0.001);
    assert!(engine
        .snapshot()
        .master_level
        .iter()
        .all(|v| *v > 0.01 && *v <= 1.));
    let bytes = std::fs::read(&path).unwrap();
    assert!(engine.start_recording(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn held_cue_is_monitor_only_and_returns_to_cue_on_release() {
    let engine = test_engine(48_000);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("master.wav");
    engine
        .dispatch(Command::SetCrossfader { value: -1. })
        .unwrap();
    engine.start_recording(&path).unwrap();
    let (producer, mut consumer) = rtrb::RingBuffer::new(16_384);
    engine.rt.lock().unwrap().cue_output = Some(device::CueWriter::new(producer, 48_000, 48_000));
    engine
        .dispatch(Command::BeginCuePreview {
            deck: DeckId::A,
            frame: 12_000,
        })
        .unwrap();
    let master = engine.render_offline(4096);
    assert!(master.iter().all(|sample| *sample == 0.));
    let headphones: Vec<_> = std::iter::from_fn(|| consumer.pop().ok())
        .flatten()
        .collect();
    assert!(energy(&headphones) > 0.001);
    assert!(engine.snapshot().decks[0].cue_previewing);
    engine
        .dispatch(Command::EndCuePreview { deck: DeckId::A })
        .unwrap();
    // Commands are consumed at the next audio block boundary.
    let release = engine.render_offline(512);
    let deck = &engine.snapshot().decks[0];
    assert!(!deck.playing);
    assert!(!deck.cue_previewing);
    assert_eq!(deck.frame, 12_000);
    // The release envelope is also isolated from speakers and the recording.
    assert!(release.iter().all(|sample| *sample == 0.));
    assert_eq!(engine.snapshot().decks[0].frame, 12_000);
    engine.stop_recording().unwrap();
    assert!(hound::WavReader::open(path)
        .unwrap()
        .samples::<f32>()
        .all(|sample| sample.unwrap() == 0.));
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    assert!(energy(&engine.render_offline(4096)) > 0.001);
}

#[test]
fn dropping_engine_finalizes_recording() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("closing.wav");
    {
        let engine = test_engine(44_100);
        engine.start_recording(&path).unwrap();
        engine.render_offline(1024);
    }
    let wav = hound::WavReader::open(path).unwrap();
    assert_eq!(wav.duration(), 1024);
    assert_eq!(wav.spec().sample_rate, 44_100);
}

#[test]
fn loading_a_track_restores_unity_trim_after_automation() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::SetChannelGain {
            deck: DeckId::A,
            db: -96.,
        })
        .unwrap();
    let buffer = sine_buffer(48_000, 440., 1.);
    let wave = Arc::new(crate::compute_preview_waveform(&buffer, 256));
    engine
        .load_buffer_if(
            DeckId::A,
            TrackId(4),
            buffer,
            wave,
            String::new(),
            String::new(),
            || true,
        )
        .unwrap();
    assert_eq!(engine.snapshot().decks[0].gain_db, 0.);
}
