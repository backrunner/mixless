use super::*;
use mixless_protocol::{Command, EngineSnapshot};

fn cc(target: MidiTarget, mode: MidiControlMode) -> MidiBinding {
    MidiBinding {
        device_id: None,
        kind: MidiSourceKind::Cc,
        channel: 2,
        number: 10,
        target,
        mode,
    }
}

#[test]
fn targeted_learning_ignores_touch_notes_releases_and_encoder_idle() {
    let mut input = InputState::default();
    input.start_learning(Some(MidiTarget::Jog { deck: DeckId::A }));
    for message in [
        [0x92, 10, 127],
        [0x82, 10, 0],
        [0xb2, 11, 64],
        [0xb2, 11, 0],
    ] {
        input.receive(&[], "controller", &message);
        assert!(input.learned.is_none());
    }
    input.receive(&[], "controller", &[0xb2, 11, 127]);
    assert_eq!(input.learned.as_ref().unwrap().kind, MidiSourceKind::Cc);
    assert_eq!(input.learned.as_ref().unwrap().number, 11);

    input.start_learning(Some(MidiTarget::Play { deck: DeckId::B }));
    for message in [[0x82, 40, 127], [0x92, 40, 0], [0xb2, 40, 0], [0xf8, 0, 0]] {
        input.receive(&[], "controller", &message);
        assert!(input.learned.is_none());
    }
    input.receive(&[], "controller", &[0x92, 40, 100]);
    input.receive(&[], "another controller", &[0x92, 41, 127]);
    assert_eq!(input.learned.as_ref().unwrap().device_id, "controller");
    assert_eq!(input.learned.as_ref().unwrap().number, 40);
    assert!(input.queued.is_empty());
}

#[test]
fn targeted_learning_accepts_zero_for_faders_and_retargeting_discards_old_capture() {
    let mut input = InputState::default();
    let map = [cc(MidiTarget::Master, MidiControlMode::Auto)];
    input.start_learning(Some(MidiTarget::Master));
    input.receive(&map, "controller", &[0x92, 10, 127]);
    assert!(input.learned.is_none());
    input.receive(&map, "controller", &[0xb2, 10, 0]);
    assert_eq!(input.learned.as_ref().unwrap().value, 0);
    assert!(input.queued.is_empty());
    input.start_learning(Some(MidiTarget::CueGain));
    assert!(input.learned.is_none());
    input.receive(&map, "controller", &[0xb2, 12, 50]);
    assert_eq!(input.learned.as_ref().unwrap().number, 12);
}

#[test]
fn learned_reassignment_persists_and_preserves_other_devices_and_assignments() {
    let dir = std::env::temp_dir().join(format!("mixless-midi-quick-{}", std::process::id()));
    let path = dir.join("map.json");
    let config = MidiConfig {
        enabled: false,
        input_ids: None,
    };
    let hub = MidiHub::start_with_config(path.clone(), config.clone()).unwrap();
    let source = MidiBinding {
        device_id: Some("one".into()),
        ..cc(MidiTarget::Master, MidiControlMode::Auto)
    };
    let other_device = MidiBinding {
        device_id: Some("two".into()),
        ..source.clone()
    };
    let old_assignment = MidiBinding {
        number: 15,
        target: MidiTarget::CueGain,
        ..source.clone()
    };
    let extra_assignment = MidiBinding {
        number: 16,
        ..old_assignment.clone()
    };
    for binding in [&source, &other_device, &old_assignment, &extra_assignment] {
        hub.upsert(binding.clone()).unwrap();
    }
    {
        let mut input = hub.input.lock().unwrap();
        input.start_learning(Some(MidiTarget::CueGain));
        input.receive(&hub.bindings(), "one", &[0xb2, 10, 90]);
    }
    let captured = hub.learned().unwrap();
    let learned_binding = MidiBinding {
        device_id: Some(captured.device_id),
        kind: captured.kind,
        channel: captured.channel,
        number: captured.number,
        target: MidiTarget::CueGain,
        mode: MidiControlMode::Auto,
    };
    hub.replace(2, learned_binding.clone()).unwrap();
    assert!(hub.drain().is_empty());
    hub.set_learn(false);
    assert!(hub.learned().is_none());
    let reopened = MidiHub::start_with_config(path.clone(), config).unwrap();
    assert_eq!(
        reopened.bindings(),
        vec![other_device, extra_assignment, learned_binding]
    );
    assert!(reopened.learn_target(MidiTarget::Master).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn catalogue_is_complete_unique_and_validates_every_default_binding() {
    let targets = MidiTarget::all();
    assert!(targets.len() > 100);
    for (index, target) in targets.iter().enumerate() {
        assert!(!targets[..index].contains(target), "duplicate {target:?}");
        validate(&cc(target.clone(), MidiControlMode::Auto)).unwrap();
    }
    assert!(targets.contains(&MidiTarget::BrowseTracks));
    for deck in [DeckId::A, DeckId::B] {
        assert!(targets.contains(&MidiTarget::LoadSelected { deck }));
        assert!(targets.contains(&MidiTarget::DeckGain { deck }));
    }
}

#[test]
fn browse_encoders_decode_both_directions_acceleration_and_neutral() {
    for (mode, plus, minus, idle) in [
        (MidiControlMode::Auto, 1, 127, 0),
        (MidiControlMode::RelativeTwosComplement, 1, 127, 64),
        (MidiControlMode::RelativeBinaryOffset, 65, 63, 64),
        (MidiControlMode::RelativeSignedBit, 1, 65, 64),
    ] {
        for target in [MidiTarget::BrowseTracks, MidiTarget::BrowsePlaylists] {
            let map = [cc(target.clone(), mode)];
            for (raw, expected) in [(plus, 1), (minus, -1)] {
                assert_eq!(
                    resolve(&map, "port", &[0xb2, 10, raw]),
                    Some(MidiAction {
                        target: target.clone(),
                        value: MidiValue::Relative(expected)
                    })
                );
            }
            assert!(resolve(&map, "port", &[0xb2, 10, idle]).is_none());
        }
    }
    let map = [cc(
        MidiTarget::BrowseTracks,
        MidiControlMode::RelativeTwosComplement,
    )];
    assert_eq!(
        resolve(&map, "port", &[0xb2, 10, 5]).unwrap().value,
        MidiValue::Relative(5)
    );
    assert_eq!(
        resolve(&map, "port", &[0xb2, 10, 123]).unwrap().value,
        MidiValue::Relative(-5)
    );
    assert!(validate(&cc(MidiTarget::BrowseTracks, MidiControlMode::Absolute)).is_err());
}

#[test]
fn legacy_json_keeps_trim_and_relative_jog_semantics() {
    let legacy = r#"{"bindings":[{"kind":"cc","channel":2,"number":10,"target":{"type":"gain","deck":"a"}},{"kind":"cc","channel":2,"number":11,"target":{"type":"jog","deck":"b"}}]}"#;
    let map: MidiMapFile = serde_json::from_str(legacy).unwrap();
    let snapshot = EngineSnapshot::default();
    assert!(matches!(
        resolve(&map.bindings, "port", &[0xb2, 10, 127])
            .unwrap()
            .command(&snapshot),
        Some(Command::SetChannelGain {
            deck: DeckId::A,
            db: 12.
        })
    ));
    assert!(matches!(
        resolve(&map.bindings, "port", &[0xb2, 11, 127])
            .unwrap()
            .command(&snapshot),
        Some(Command::Jog {
            deck: DeckId::B,
            delta_frames: -16.
        })
    ));
}

#[test]
fn relative_gain_starts_at_effective_auto_value_and_is_bounded() {
    let map = [cc(
        MidiTarget::DeckGain { deck: DeckId::B },
        MidiControlMode::RelativeTwosComplement,
    )];
    let mut snapshot = EngineSnapshot::default();
    snapshot.decks[1].limiter_gain_db = 2.;
    snapshot.decks[1].automix_gain_db = 3.;
    let up = resolve(&map, "port", &[0xb2, 10, 1]).unwrap();
    let Some(Command::SetDeckLimiterGain {
        deck: DeckId::B,
        db,
    }) = up.command(&snapshot)
    else {
        panic!()
    };
    assert!((db - (5. + 24. / 127.)).abs() < 1e-5);
    snapshot.decks[1].limiter_gain_db = 12.;
    assert!(matches!(
        up.command(&snapshot),
        Some(Command::SetDeckLimiterGain { db: 12., .. })
    ));
}

#[test]
fn momentary_controls_deliver_release_and_learning_releases_held_controls() {
    let binding = cc(
        MidiTarget::JogTouch { deck: DeckId::A },
        MidiControlMode::Auto,
    );
    let mut input = InputState::default();
    input.receive(&[binding.clone()], "port", &[0xb2, 10, 127]);
    input.receive(&[binding.clone()], "port", &[0xb2, 10, 127]);
    assert_eq!(input.queued.len(), 1);
    input.receive(&[binding], "port", &[0xb2, 10, 0]);
    assert_eq!(input.queued.last().unwrap().value, MidiValue::Release);
    assert!(input.held.is_empty());

    let dir = std::env::temp_dir().join(format!("mixless-midi-hold-{}", std::process::id()));
    let hub = MidiHub::start_with_config(
        dir.join("map.json"),
        MidiConfig {
            enabled: false,
            input_ids: None,
        },
    )
    .unwrap();
    let brake = cc(MidiTarget::Brake { deck: DeckId::B }, MidiControlMode::Auto);
    hub.input
        .lock()
        .unwrap()
        .receive(&[brake], "port", &[0xb2, 10, 127]);
    hub.drain();
    hub.set_learn(true);
    hub.set_learn(false);
    hub.set_learn(false);
    assert_eq!(
        hub.drain(),
        vec![MidiAction {
            target: MidiTarget::Brake { deck: DeckId::B },
            value: MidiValue::Release
        }]
    );
}

#[test]
fn note_off_releases_holds_but_does_not_load_or_move_the_library() {
    for target in [
        MidiTarget::LoadFocused,
        MidiTarget::TrackNext,
        MidiTarget::PlaylistPrevious,
    ] {
        let binding = MidiBinding {
            kind: MidiSourceKind::Note,
            ..cc(target.clone(), MidiControlMode::Auto)
        };
        assert!(resolve(&[binding.clone()], "port", &[0x82, 10, 127]).is_none());
        assert_eq!(
            resolve(&[binding], "port", &[0x92, 10, 127])
                .unwrap()
                .target,
            target
        );
    }
    let binding = MidiBinding {
        kind: MidiSourceKind::Note,
        ..cc(MidiTarget::Brake { deck: DeckId::A }, MidiControlMode::Auto)
    };
    assert_eq!(
        resolve(&[binding], "port", &[0x82, 10, 127]).unwrap().value,
        MidiValue::Release
    );
}

#[test]
fn invalid_slots_and_relative_buttons_are_rejected() {
    for target in [
        MidiTarget::FxMix {
            deck: DeckId::A,
            slot: 4,
        },
        MidiTarget::ClearCue {
            deck: DeckId::B,
            index: 8,
        },
    ] {
        assert!(validate(&cc(target, MidiControlMode::Auto)).is_err());
    }
    assert!(validate(&cc(
        MidiTarget::Play { deck: DeckId::A },
        MidiControlMode::RelativeSignedBit
    ))
    .is_err());
}

#[test]
fn remapping_and_disabling_release_the_previous_held_target() {
    let root = std::env::temp_dir().join(format!("mixless-midi-remap-{}", std::process::id()));
    let config = MidiConfig {
        enabled: false,
        input_ids: None,
    };
    let hub = MidiHub::start_with_config(root.join("map.json"), config.clone()).unwrap();
    let brake = cc(MidiTarget::Brake { deck: DeckId::A }, MidiControlMode::Auto);
    hub.upsert(brake.clone()).unwrap();
    hub.input
        .lock()
        .unwrap()
        .receive(&[brake.clone()], "port", &[0xb2, 10, 127]);
    hub.drain();
    hub.replace(0, cc(MidiTarget::Master, MidiControlMode::Auto))
        .unwrap();
    assert_eq!(
        hub.drain(),
        vec![MidiAction {
            target: brake.target.clone(),
            value: MidiValue::Release
        }]
    );
    hub.input
        .lock()
        .unwrap()
        .receive(&[brake.clone()], "port", &[0xb2, 10, 127]);
    hub.drain();
    hub.configure(config).unwrap();
    assert_eq!(
        hub.drain(),
        vec![MidiAction {
            target: brake.target,
            value: MidiValue::Release
        }]
    );
    fs::remove_dir_all(root).unwrap();
}
