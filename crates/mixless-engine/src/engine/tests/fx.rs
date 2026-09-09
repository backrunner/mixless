use super::*;

#[test]
fn insert_commands_change_audio_and_bypass_restores_dry() {
    for deck in [DeckId::A, DeckId::B] {
        for slot in FxSlot::INSERTS {
            let engine = test_engine(48_000);
            engine.dispatch(Command::PlayPause { deck }).unwrap();
            engine.render_offline(1000);
            engine
                .dispatch(Command::SetFx {
                    deck: Some(deck),
                    slot,
                    params: FxParams {
                        kind: Some("gate".into()),
                        mix: 1.0,
                        time_beats: Some(0.25),
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
            let snapshot = engine.snapshot();
            for channel in [DeckId::A, DeckId::B] {
                for (index, insert) in FxSlot::INSERTS.into_iter().enumerate() {
                    assert_eq!(
                        snapshot.decks[channel.index()].insert[index],
                        (channel == deck && insert == slot).then_some(slot),
                        "{deck:?} {slot:?} must only enable its own insert"
                    );
                }
            }
            let mut gated = 0.0;
            for _ in 0..100 {
                gated += energy(&engine.render_offline(256));
            }
            engine
                .dispatch(Command::SetFxBypass {
                    deck,
                    slot,
                    on: true,
                })
                .unwrap();
            assert_eq!(
                engine.snapshot().decks[deck.index()].insert,
                [None; FxSlot::INSERTS.len()]
            );
            engine.render_offline(1000);
            let mut dry = 0.0;
            for _ in 0..100 {
                dry += energy(&engine.render_offline(256));
            }
            assert!(
                gated > dry * 0.3 && gated < dry * 0.7,
                "{deck:?} {slot:?}: gate {gated}, dry {dry}"
            );
        }
    }
}

#[test]
fn invalid_fx_parameters_are_rejected() {
    let engine = test_engine(48_000);
    assert!(engine
        .dispatch(Command::SetFilterResonance {
            deck: DeckId::A,
            resonance: f32::NAN
        })
        .is_err());
    assert!(engine
        .dispatch(Command::Jog {
            deck: DeckId::A,
            delta_frames: f32::INFINITY
        })
        .is_err());
    assert!(engine
        .dispatch(Command::SetFx {
            deck: Some(DeckId::A),
            slot: FxSlot::Insert0,
            params: FxParams {
                kind: Some("not-an-effect".into()),
                ..FxParams::default()
            }
        })
        .is_err());
}

#[test]
fn cutoff_command_and_snapshot_agree() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::SetFilter {
            deck: DeckId::A,
            cutoff_hz: 800.0,
            kind: FilterKind::Lp,
        })
        .unwrap();
    assert!((engine.snapshot().decks[0].lp_hz - 800.0).abs() < 15.0);
    engine
        .dispatch(Command::SetFilter {
            deck: DeckId::A,
            cutoff_hz: 1500.0,
            kind: FilterKind::Hp,
        })
        .unwrap();
    assert!((engine.snapshot().decks[0].hp_hz - 1500.0).abs() < 25.0);
}

#[test]
fn all_effects_and_sweeps_are_finite_at_supported_rates() {
    for sample_rate in [44_100, 48_000, 96_000] {
        let engine = test_engine(sample_rate);
        for deck in [DeckId::A, DeckId::B] {
            engine.dispatch(Command::PlayPause { deck }).unwrap();
            engine
                .dispatch(Command::SetFilterResonance {
                    deck,
                    resonance: 1.0,
                })
                .unwrap();
            engine
                .dispatch(Command::SetFxSend { deck, value: 1.0 })
                .unwrap();
            for (index, slot) in FxSlot::INSERTS.into_iter().enumerate() {
                let kind = if deck == DeckId::A {
                    ["echo", "flanger", "gate", "reverb"][index]
                } else {
                    ["reverb", "phaser", "echo", "gate"][index]
                };
                engine
                    .dispatch(Command::SetFx {
                        deck: Some(deck),
                        slot,
                        params: FxParams {
                            kind: Some(kind.into()),
                            mix: 0.8,
                            feedback: Some(0.88),
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
        for block in 0..500 {
            engine
                .dispatch(Command::SetChannelFilter {
                    deck: DeckId::A,
                    amount: (block as f32 * 0.07).sin(),
                })
                .unwrap();
            let output = engine.render_offline(256);
            assert!(output
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() <= 1.0));
        }
        assert_eq!(engine.snapshot().xrun_count, 0);
    }
}
