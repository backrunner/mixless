use super::*;

fn monitor(engine: &Engine, rate: u32) -> rtrb::Consumer<[f32; 2]> {
    let (producer, consumer) = rtrb::RingBuffer::new(16_384);
    engine.rt.lock().unwrap().cue_output = Some(device::CueWriter::new(producer, rate, rate));
    consumer
}

fn drain(consumer: &mut rtrb::Consumer<[f32; 2]>) -> Vec<f32> {
    std::iter::from_fn(|| consumer.pop().ok())
        .flatten()
        .collect()
}

#[test]
fn relative_and_absolute_scrubs_route_by_playback_without_changing_pfl() {
    for rate in [44_100, 48_000, 96_000] {
        for deck in [DeckId::A, DeckId::B] {
            for absolute in [false, true] {
                for playing in [false, true] {
                    let engine = test_engine(rate);
                    let mut cue = monitor(&engine, rate);
                    engine
                        .dispatch(Command::Seek {
                            deck,
                            frame: rate as u64,
                        })
                        .unwrap();
                    if playing {
                        engine.dispatch(Command::PlayPause { deck }).unwrap();
                    }
                    engine.render_offline(256);
                    drain(&mut cue);
                    engine
                        .dispatch(Command::SetJogTouch {
                            deck,
                            touching: true,
                        })
                        .unwrap();
                    engine
                        .dispatch(if absolute {
                            Command::ScrubTo {
                                deck,
                                frame: rate as u64 * 3,
                            }
                        } else {
                            Command::Jog {
                                deck,
                                delta_frames: rate as f32 * 0.05,
                            }
                        })
                        .unwrap();
                    let master = engine.render_offline(rate as usize / 40);
                    let headphones = drain(&mut cue);
                    if playing {
                        assert!(energy(&master) > 0.0001, "playing scrub is silent");
                        assert!(energy(&headphones) < 1e-10, "PFL was enabled implicitly");
                    } else {
                        assert!(
                            master.iter().all(|v| *v == 0.),
                            "paused scrub leaked to master"
                        );
                        assert!(energy(&headphones) > 0.0001, "paused scrub is silent");
                    }
                    assert!(
                        master
                            .iter()
                            .chain(&headphones)
                            .all(|v| v.is_finite() && v.abs() <= 0.98)
                    );
                    let state = engine.snapshot();
                    assert_eq!(state.deck(deck).playing, playing);
                    assert!(!state.deck(deck).pfl);
                    assert_eq!(state.decks[1 - deck.index()].frame, 0);
                    engine
                        .dispatch(Command::SetJogTouch {
                            deck,
                            touching: false,
                        })
                        .unwrap();
                    let tail = engine.render_offline(rate as usize / 20);
                    if !playing {
                        assert!(tail.iter().all(|v| *v == 0.), "release tail leaked");
                        drain(&mut cue);
                        engine.render_offline(1024);
                        assert!(energy(&drain(&mut cue)) < 1e-10);
                    }
                }
            }
        }
    }
}

#[test]
fn paused_scrub_is_pre_fader_and_has_no_master_fallback() {
    for connected in [false, true] {
        let engine = test_engine(48_000);
        let mut cue = connected.then(|| monitor(&engine, 48_000));
        engine
            .dispatch(Command::SetChannelFader {
                deck: DeckId::A,
                value: 0.,
            })
            .unwrap();
        engine
            .dispatch(Command::SetCrossfader { value: 1. })
            .unwrap();
        engine.dispatch(Command::SetMaster { value: 0. }).unwrap();
        engine.render_offline(1024);
        if let Some(cue) = &mut cue {
            drain(cue);
        }
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: true,
            })
            .unwrap();
        engine
            .dispatch(Command::Jog {
                deck: DeckId::A,
                delta_frames: 2400.,
            })
            .unwrap();
        assert!(engine.render_offline(1024).iter().all(|v| *v == 0.));
        if let Some(cue) = &mut cue {
            assert!(energy(&drain(cue)) > 0.001);
        }
        engine
            .dispatch(Command::SetChannelFader {
                deck: DeckId::A,
                value: 1.,
            })
            .unwrap();
        engine
            .dispatch(Command::SetCrossfader { value: -1. })
            .unwrap();
        engine.dispatch(Command::SetMaster { value: 1. }).unwrap();
        engine
            .dispatch(Command::Jog {
                deck: DeckId::A,
                delta_frames: 4800.,
            })
            .unwrap();
        assert!(engine.render_offline(2048).iter().all(|v| *v == 0.));
    }
}

#[test]
fn switching_playback_during_a_scrub_changes_bus_and_release_does_not_leak() {
    let engine = test_engine(48_000);
    let mut cue = monitor(&engine, 48_000);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    for playing in [false, true, false] {
        if engine.snapshot().decks[0].playing != playing {
            engine
                .dispatch(Command::PlayPause { deck: DeckId::A })
                .unwrap();
        }
        engine
            .dispatch(Command::Jog {
                deck: DeckId::A,
                delta_frames: 2400.,
            })
            .unwrap();
        let master = engine.render_offline(1024);
        let headphones = drain(&mut cue);
        assert_eq!(energy(&master) > 0.001, playing);
        assert_eq!(energy(&headphones) > 0.001, !playing);
    }
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: false,
        })
        .unwrap();
    assert!(engine.render_offline(4096).iter().all(|v| *v == 0.));
    drain(&mut cue);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    assert!(energy(&engine.render_offline(1024)) > 0.001);
    assert!(energy(&drain(&mut cue)) < 1e-10);
}

#[test]
fn progress_scrub_auditions_near_target_and_commits_even_in_slip_mode() {
    let engine = test_engine(48_000);
    let mut cue = monitor(&engine, 48_000);
    engine
        .dispatch(Command::SetVinylMode {
            deck: DeckId::A,
            vinyl: true,
            slip: true,
        })
        .unwrap();
    for playing in [false, true] {
        if playing {
            engine
                .dispatch(Command::PlayPause { deck: DeckId::A })
                .unwrap();
        }
        for target in [144_000, 24_000, 190_000, 0] {
            engine
                .dispatch(Command::SetJogTouch {
                    deck: DeckId::A,
                    touching: true,
                })
                .unwrap();
            engine
                .dispatch(Command::ScrubTo {
                    deck: DeckId::A,
                    frame: target,
                })
                .unwrap();
            let master = engine.render_offline(512);
            let headphones = drain(&mut cue);
            assert!(engine.snapshot().decks[0].frame.abs_diff(target) < 1920);
            assert!(energy(if playing { &master } else { &headphones }) > 0.0001);
            engine
                .dispatch(Command::SetJogTouch {
                    deck: DeckId::A,
                    touching: false,
                })
                .unwrap();
            engine.render_offline(512);
            let state = engine.snapshot();
            assert!(state.decks[0].frame.abs_diff(target) <= if playing { 520 } else { 1 });
            assert_eq!(state.decks[0].playing, playing);
            drain(&mut cue);
        }
    }
}

#[test]
fn paused_scrub_and_its_insert_tail_do_not_change_the_other_decks_master_pcm() {
    let baseline = test_engine(48_000);
    let engine = test_engine(48_000);
    let mut cue = monitor(&engine, 48_000);
    for engine in [&baseline, &engine] {
        engine
            .dispatch(Command::PlayPause { deck: DeckId::B })
            .unwrap();
        engine
            .dispatch(Command::SetFx {
                deck: Some(DeckId::A),
                slot: FxSlot::Insert0,
                params: FxParams {
                    kind: Some("echo".into()),
                    mix: 0.5,
                    time_beats: Some(0.125),
                    feedback: Some(0.6),
                    ..Default::default()
                },
            })
            .unwrap();
        engine
            .dispatch(Command::SetFxBypass {
                deck: DeckId::A,
                slot: FxSlot::Insert0,
                on: false,
            })
            .unwrap();
    }
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    engine
        .dispatch(Command::Jog {
            deck: DeckId::A,
            delta_frames: 4800.,
        })
        .unwrap();
    let mut tail_energy = 0.;
    for block in 0..20 {
        if block == 2 {
            engine
                .dispatch(Command::SetJogTouch {
                    deck: DeckId::A,
                    touching: false,
                })
                .unwrap();
        }
        let actual = engine.render_offline(1024);
        assert_eq!(
            actual,
            baseline.render_offline(1024),
            "paused scrub altered master PCM at block {block}"
        );
        if block > 2 {
            tail_energy += energy(&drain(&mut cue));
        } else {
            drain(&mut cue);
        }
    }
    assert!(
        tail_energy > 0.0001,
        "insert tail did not remain on monitor"
    );
}

#[test]
fn playing_scrub_preserves_explicit_pfl_and_cue_preview_stays_on_monitor() {
    let engine = test_engine(48_000);
    let mut cue = monitor(&engine, 48_000);
    engine
        .dispatch(Command::SetPfl {
            deck: DeckId::A,
            on: true,
        })
        .unwrap();
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    engine
        .dispatch(Command::Jog {
            deck: DeckId::A,
            delta_frames: 2400.,
        })
        .unwrap();
    assert!(energy(&engine.render_offline(1024)) > 0.001);
    assert!(energy(&drain(&mut cue)) > 0.001);
    assert!(engine.snapshot().decks[0].pfl);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: false,
        })
        .unwrap();
    engine
        .dispatch(Command::SetPfl {
            deck: DeckId::A,
            on: false,
        })
        .unwrap();
    engine
        .dispatch(Command::BeginCuePreview {
            deck: DeckId::A,
            frame: 24_000,
        })
        .unwrap();
    engine.render_offline(512);
    drain(&mut cue);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    engine
        .dispatch(Command::ScrubTo {
            deck: DeckId::A,
            frame: 96_000,
        })
        .unwrap();
    assert!(engine.render_offline(1024).iter().all(|v| *v == 0.));
    assert!(energy(&drain(&mut cue)) > 0.001);
}

#[test]
fn releasing_relative_scrub_at_the_start_keeps_forward_playback() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::Seek {
            deck: DeckId::A,
            frame: 2400,
        })
        .unwrap();
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine.render_offline(512);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    engine
        .dispatch(Command::Jog {
            deck: DeckId::A,
            delta_frames: -4800.,
        })
        .unwrap();
    engine.render_offline(128);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: false,
        })
        .unwrap();
    engine.render_offline(512);
    let state = engine.snapshot();
    assert!(state.decks[0].playing);
    assert!((500..=520).contains(&state.decks[0].frame));
}
