use super::*;
use mixless_protocol::StemKind;

pub(super) fn stem_fixture(sr: u32, isolated: bool, attach: bool) -> Engine {
    let engine = test_engine(48000);
    for deck in [DeckId::A, DeckId::B] {
        let frames = sr as usize * 12;
        let mut vocals = Vec::with_capacity(frames * 2);
        let mut drums = Vec::with_capacity(frames * 2);
        let mut mix = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f32 / sr as f32;
            let v = 0.12 * (std::f32::consts::TAU * 440. * t).sin();
            let d = 0.08 * (std::f32::consts::TAU * 80. * t).sin();
            let instrument = 0.10 * (std::f32::consts::TAU * 880. * t).sin();
            vocals.extend([v, v * 0.8]);
            drums.extend([d, d]);
            mix.extend(if isolated {
                [v, v * 0.8]
            } else {
                [v + d + instrument, v * 0.8 + d + instrument]
            });
        }
        let source = Arc::new(AudioBuffer {
            samples: mix,
            frames: frames as u64,
            sample_rate: sr,
            loudness: Default::default(),
        });
        let wave = Arc::new(crate::compute_preview_waveform(&source, 32));
        engine
            .load_buffer_if(
                deck,
                TrackId(deck.index() as i64 + 1),
                source.clone(),
                wave,
                "fixture".into(),
                "".into(),
                || true,
            )
            .unwrap();
        if attach {
            engine
                .attach_stems(
                    deck,
                    TrackId(deck.index() as i64 + 1),
                    &source,
                    Arc::new(crate::StemBuffer::new(sr, vocals, drums).unwrap()),
                )
                .unwrap();
        }
    }
    engine
        .dispatch(Command::SetCrossfader { value: -1. })
        .unwrap();
    engine
}

#[test]
fn unity_stems_are_the_original_with_resampling_pitch_and_tempo() {
    for sr in [44100, 48000] {
        let dry = stem_fixture(sr, false, false);
        let stems = stem_fixture(sr, false, true);
        for engine in [&dry, &stems] {
            engine
                .dispatch(Command::SetRate {
                    deck: DeckId::A,
                    rate: 1.12,
                })
                .unwrap();
            engine
                .dispatch(Command::SetPitchSemitones {
                    deck: DeckId::A,
                    semitones: 2.,
                })
                .unwrap();
            engine
                .dispatch(Command::PlayPause { deck: DeckId::A })
                .unwrap();
        }
        for _ in 0..400 {
            let a = dry.render_offline(128);
            let b = stems.render_offline(128);
            assert!(a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-6));
        }
        assert_eq!(
            dry.snapshot().decks[0].frame,
            stems.snapshot().decks[0].frame
        );
    }
}

#[test]
fn isolated_voice_shares_cues_loops_reverse_and_keylock_clock() {
    let reference = stem_fixture(48000, true, false);
    let stems = stem_fixture(48000, false, true);
    for stem in [StemKind::Drums, StemKind::Instruments] {
        stems
            .dispatch(Command::SetStemGain {
                deck: DeckId::A,
                stem,
                value: 0.,
            })
            .unwrap();
    }
    for e in [&reference, &stems] {
        e.dispatch(Command::SetRate {
            deck: DeckId::A,
            rate: 1.08,
        })
        .unwrap();
        e.dispatch(Command::SetPitchSemitones {
            deck: DeckId::A,
            semitones: -2.,
        })
        .unwrap();
        e.dispatch(Command::SetCue {
            deck: DeckId::A,
            index: 0,
            frame: 96000,
        })
        .unwrap();
        e.dispatch(Command::PlayPause { deck: DeckId::A }).unwrap();
        e.render_offline(48000);
    }
    for command in [
        Command::JumpCue {
            deck: DeckId::A,
            index: 0,
        },
        Command::SetLoopBeats {
            deck: DeckId::A,
            beats: 2.,
            on: true,
        },
        Command::SetReverse {
            deck: DeckId::A,
            on: true,
        },
    ] {
        for e in [&reference, &stems] {
            e.dispatch(command.clone()).unwrap();
        }
        // Ignore transition history, then compare the actual stem against the
        // same source rendered through the original engine path.
        reference.render_offline(12000);
        stems.render_offline(12000);
        let a = reference.render_offline(24000);
        let b = stems.render_offline(24000);
        let error = a.iter().zip(&b).map(|(a, b)| (a - b).powi(2)).sum::<f32>() / a.len() as f32;
        assert!(error < 1e-8, "isolated source RMS error {}", error.sqrt());
        assert_eq!(
            reference.snapshot().decks[0].frame,
            stems.snapshot().decks[0].frame
        );
    }
}

#[test]
fn missing_stale_or_contended_stems_keep_transport_safe_and_all_mute_is_silent() {
    let engine = stem_fixture(48000, false, true);
    let source = engine.stem_source(DeckId::A, TrackId(1)).unwrap();
    let stems = Arc::new(
        crate::StemBuffer::new(
            48000,
            vec![0.; source.samples.len()],
            vec![0.; source.samples.len()],
        )
        .unwrap(),
    );
    assert!(!engine
        .attach_stems(DeckId::A, TrackId(99), &source, stems.clone())
        .unwrap());
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine.render_offline(1000);
    let before = engine.snapshot().decks[0].frame;
    {
        let _guard = engine.shared.decks[0].stems.lock().unwrap();
        let audio = engine.render_offline(1000);
        assert!(energy(&audio) > 0.001);
    }
    assert!(engine.snapshot().decks[0].frame > before);
    for stem in [StemKind::Vocals, StemKind::Drums, StemKind::Instruments] {
        engine
            .dispatch(Command::SetStemGain {
                deck: DeckId::A,
                stem,
                value: 0.,
            })
            .unwrap();
    }
    engine.render_offline(24000);
    assert!(energy(&engine.render_offline(24000)) < 1e-12);
    engine.eject(DeckId::A);
    assert!(!engine.stems_ready(DeckId::A));
    assert!(!engine
        .attach_stems(DeckId::A, TrackId(1), &source, stems)
        .unwrap());
    assert!(engine
        .dispatch(Command::SetStemGain {
            deck: DeckId::A,
            stem: StemKind::Vocals,
            value: f32::NAN
        })
        .is_err());
}

#[test]
#[ignore = "Serial release budget: two decks, active stems, keylock/pitch, loops, FX, recording and retriggered cues"]
fn stems_callback_budget() {
    let engine = stem_fixture(48000, false, true);
    engine
        .dispatch(Command::SetCrossfader { value: 0. })
        .unwrap();
    for deck in [DeckId::A, DeckId::B] {
        engine
            .dispatch(Command::SetStemGain {
                deck,
                stem: StemKind::Vocals,
                value: 0.3,
            })
            .unwrap();
        engine
            .dispatch(Command::SetStemGain {
                deck,
                stem: StemKind::Drums,
                value: 0.6,
            })
            .unwrap();
        engine
            .dispatch(Command::SetRate {
                deck,
                rate: if deck == DeckId::A { 1.08 } else { 0.96 },
            })
            .unwrap();
        engine
            .dispatch(Command::SetPitchSemitones {
                deck,
                semitones: if deck == DeckId::A { 2. } else { -1. },
            })
            .unwrap();
        engine
            .dispatch(Command::SetCue {
                deck,
                index: 0,
                frame: 96000,
            })
            .unwrap();
        engine
            .dispatch(Command::SetLoopBeats {
                deck,
                beats: 8.,
                on: true,
            })
            .unwrap();
        let mut params = mixless_protocol::FxParams::default();
        params.kind = Some("reverb".into());
        params.mix = 0.25;
        engine
            .dispatch(Command::SetFx {
                deck: Some(deck),
                slot: FxSlot::INSERTS[0],
                params,
            })
            .unwrap();
        engine.dispatch(Command::PlayPause { deck }).unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    engine
        .start_recording(&dir.path().join("stems.wav"))
        .unwrap();
    let mut rt = engine.rt.lock().unwrap();
    let mut output = [0f32; 256];
    for _ in 0..1000 {
        engine.shared.process_block(&mut rt, &mut output, 2);
    }
    let mut timings = Vec::with_capacity(4000);
    for i in 0..4000 {
        if i % 64 == 0 {
            for deck in [DeckId::A, DeckId::B] {
                engine
                    .dispatch(Command::JumpCue { deck, index: 0 })
                    .unwrap();
            }
        }
        let start = std::time::Instant::now();
        engine.shared.process_block(&mut rt, &mut output, 2);
        timings.push(start.elapsed().as_secs_f64() * 1000.);
        assert!(output.iter().all(|v| v.is_finite() && v.abs() <= 1.));
    }
    drop(rt);
    engine.stop_recording().unwrap();
    timings.sort_by(f64::total_cmp);
    let p99 = timings[3960];
    let max = timings[3999];
    eprintln!("stems dual decks + pitch + FX + recording + cues: p99={p99:.3} ms max={max:.3} ms / 2.667 ms");
    assert!(p99 < 2.667 * 0.5, "Insufficient 128-frame callback margin");
}
