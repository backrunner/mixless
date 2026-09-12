use super::*;

#[test]
#[ignore = "release-only 60-second realtime workload benchmark"]
fn audio_callback_budget() {
    for (block_frames, scratch) in [(256, false), (128, true)] {
        let engine = test_engine(48_000);
        for deck in [DeckId::A, DeckId::B] {
            engine.dispatch(Command::PlayPause { deck }).unwrap();
            engine
                .dispatch(Command::SetLoop {
                    deck,
                    bars: 1,
                    on: true,
                })
                .unwrap();
            engine
                .dispatch(Command::SetRate {
                    deck,
                    rate: if deck == DeckId::A { 0.92 } else { 1.08 },
                })
                .unwrap();
            engine
                .dispatch(Command::SetPitchSemitones {
                    deck,
                    semitones: if deck == DeckId::A { 2.0 } else { -3.0 },
                })
                .unwrap();
            engine
                .dispatch(Command::SetChannelFilter { deck, amount: -0.3 })
                .unwrap();
            engine
                .dispatch(Command::SetFilterResonance {
                    deck,
                    resonance: 1.0,
                })
                .unwrap();
            engine
                .dispatch(Command::SetFxSend { deck, value: 0.8 })
                .unwrap();
            for (slot, kind) in [
                (FxSlot::Insert0, "flanger"),
                (FxSlot::Insert1, "phaser"),
                (FxSlot::Insert2, "reverb"),
                (FxSlot::Insert3, "echo"),
            ] {
                engine
                    .dispatch(Command::SetFx {
                        deck: Some(deck),
                        slot,
                        params: FxParams {
                            kind: Some(kind.into()),
                            mix: 0.6,
                            ..FxParams::default()
                        },
                    })
                    .unwrap();
                engine
                    .dispatch(Command::SetFxBypass {
                        deck,
                        slot,
                        on: false,
                    })
                    .unwrap();
            }
        }
        for slot in [FxSlot::SendEcho, FxSlot::SendReverb] {
            engine
                .dispatch(Command::SetFx {
                    deck: None,
                    slot,
                    params: FxParams {
                        mix: 0.6,
                        ..FxParams::default()
                    },
                })
                .unwrap();
            engine
                .dispatch(Command::SetFxBypass {
                    deck: DeckId::A,
                    slot,
                    on: false,
                })
                .unwrap();
        }
        engine.render_offline(2048);
        if scratch {
            engine
                .dispatch(Command::SetJogTouch {
                    deck: DeckId::A,
                    touching: true,
                })
                .unwrap();
            engine
                .dispatch(Command::SetJogTouch {
                    deck: DeckId::B,
                    touching: true,
                })
                .unwrap();
        }
        let mut output = vec![0.0; block_frames * 2];
        let mut timings = Vec::with_capacity(48_000 * 60 / block_frames);
        let mut rt = engine.rt.lock().unwrap();
        for block in 0..48_000 * 60 / block_frames {
            if scratch && block % 512 == 0 {
                for deck in [DeckId::A, DeckId::B] {
                    engine
                        .dispatch(Command::SetJogTouch {
                            deck,
                            touching: block % 1024 == 0,
                        })
                        .unwrap();
                }
            }
            if scratch && block % 1024 < 512 && block % 8 == 0 {
                for deck in [DeckId::A, DeckId::B] {
                    engine
                        .dispatch(Command::Jog {
                            deck,
                            delta_frames: if block % 128 < 64 { 4096.0 } else { -4096.0 },
                        })
                        .unwrap();
                }
            }
            let start = std::time::Instant::now();
            engine.shared.process_block(&mut rt, &mut output, 2);
            timings.push(start.elapsed().as_secs_f64());
            assert!(
                output
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
            );
        }
        timings.sort_by(f64::total_cmp);
        let percentile = timings[timings.len() * 99 / 100];
        let budget = block_frames as f64 / 48_000.0;
        eprintln!(
            "audio frames={block_frames} scratch={scratch} p50={:.3}ms p99={:.3}ms budget={:.3}ms ({:.1}%)",
            timings[timings.len() / 2] * 1000.0,
            percentile * 1000.0,
            budget * 1000.0,
            percentile / budget * 100.0
        );
        assert!(percentile < budget * if scratch { 0.35 } else { 0.5 });
    }
}

#[test]
#[ignore = "serial release budget for retriggered time-stretched cues"]
fn cue_callback_budget() {
    for block_frames in [128, 512] {
        let engine = test_engine(48_000);
        for deck in [DeckId::A, DeckId::B] {
            engine.dispatch(Command::PlayPause { deck }).unwrap();
            engine
                .dispatch(Command::SetRate { deck, rate: 1.08 })
                .unwrap();
            engine
                .dispatch(Command::SetPitchSemitones {
                    deck,
                    semitones: 2.,
                })
                .unwrap();
            engine.set_cue_frame(deck, 0, 24000);
            engine.set_cue_frame(deck, 1, 48000);
        }
        engine.render_offline(4096);
        let mut rt = engine.rt.lock().unwrap();
        let mut output = vec![0.; block_frames * 2];
        let mut timings = Vec::with_capacity(2000);
        for i in 0..2000 {
            for deck in [DeckId::A, DeckId::B] {
                engine
                    .dispatch(Command::JumpCue {
                        deck,
                        index: (i % 2) as u8,
                    })
                    .unwrap();
            }
            let started = std::time::Instant::now();
            engine.shared.process_block(&mut rt, &mut output, 2);
            timings.push(started.elapsed().as_secs_f64());
            assert!(output.iter().all(|s| s.is_finite() && s.abs() <= 1.));
        }
        timings.sort_by(f64::total_cmp);
        let p99 = timings[timings.len() * 99 / 100];
        let budget = block_frames as f64 / 48000.;
        eprintln!(
            "cue frames={block_frames} p50={:.3}ms p99={:.3}ms budget={:.3}ms",
            timings[timings.len() / 2] * 1000.,
            p99 * 1000.,
            budget * 1000.
        );
        assert!(p99 < budget * 0.5);
    }
}
