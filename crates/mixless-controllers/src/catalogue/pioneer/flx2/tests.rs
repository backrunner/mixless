use super::*;
use crate::{replay::feedback_packet, router::Router};
use mixless_protocol::EngineSnapshot;

fn preset() -> ControllerPreset {
    builtin_presets()
        .into_iter()
        .find(|p| p.name == "DDJ-FLX2")
        .unwrap()
}

fn route(router: &mut Router, p: &ControllerPreset, bytes: &[u8]) -> Vec<MidiAction> {
    router.route("flx2", "main", p, false, bytes, &EngineSnapshot::default())
}

#[test]
fn flx2_identity_and_coverage_do_not_inherit_flx4_controls_or_audio_order() {
    let p = preset();
    assert_eq!(p.id, "builtin-ddj-flx2-v1");
    assert_eq!(p.verification, Verification::Protocol);
    assert!(p.matches_midi("DDJ-FLX2"));
    for name in ["DDJ-FLX4", "DDJ-400", "DDJ-FLX2 MK2", "DDJ-FLX2 MIDI 2"] {
        assert!(!p.matches_midi(name));
    }
    assert!(p.audio.is_none());
    assert_eq!(p.bindings.len(), 69);
    assert_eq!(p.feedback.len(), 26);
    assert!(p.bindings.iter().all(|b| !matches!(
        b.target,
        MidiTarget::Gain { .. }
            | MidiTarget::Loop { .. }
            | MidiTarget::BrowseTracks
            | MidiTarget::LoadSelected { .. }
            | MidiTarget::Automix
            | MidiTarget::CueMix
    )));
    assert!(
        p.feedback
            .iter()
            .all(|f| !matches!(f.signal, FeedbackSignal::Meter { .. }))
    );
    assert!(p.provenance.iter().all(|s| s.url.contains("DDJ-FLX2")));
}

#[test]
fn flx2_all_hot_cues_and_shift_clear_are_separate_from_mode_selection() {
    let p = preset();
    for (ch, pad_channel, deck) in [(0, 7, DeckId::A), (1, 9, DeckId::B)] {
        let mut r = Router::default();
        for index in 0..8 {
            let bytes = [0x90 | pad_channel, index, 127];
            assert_eq!(
                route(&mut r, &p, &bytes),
                vec![MidiAction {
                    target: MidiTarget::Cue { deck, index },
                    value: MidiValue::Press,
                }]
            );
            assert!(route(&mut r, &p, &bytes).is_empty());
            assert!(route(&mut r, &p, &[0x90 | pad_channel, index, 0]).is_empty());
        }
        assert!(route(&mut r, &p, &[0x90 | ch, 0x3f, 127]).is_empty());
        for index in 0..8 {
            assert_eq!(
                route(&mut r, &p, &[0x90 | (pad_channel + 1), index, 127]),
                vec![MidiAction {
                    target: MidiTarget::ClearCue { deck, index },
                    value: MidiValue::Press,
                }]
            );
        }
        for bytes in [
            [0x90 | ch, 0x5c, 127], // Shift + Sync selects pad mode, not Sync.
            [0x90 | ch, 0, 127],
            [0x90 | ch, 5, 127],
            [0x90 | ch, 1, 127],
            [0x90 | ch, 3, 127],
            [0x90 | ch, 0x47, 127],
            [0x90 | ch, 0x48, 127],
            [0x90 | ch, 0x66, 127],
            [0x90 | ch, 0x5d, 127],
            [0x90 | ch, 0x52, 127],
            [0x90 | pad_channel, 0x10, 127], // Other pad banks are not hot cues.
            [0x90 | pad_channel, 0x60, 127],
            [0x90 | pad_channel, 0x30, 127],
        ] {
            assert!(
                route(&mut r, &p, &bytes).is_empty(),
                "unmapped message {bytes:?}"
            );
        }
    }
}

#[test]
fn flx2_14bit_controls_use_documented_pairs_on_both_decks_and_global_channel() {
    let p = preset();
    let mut cases = vec![
        (6, 0x1f, MidiTarget::Xfader),
        (6, 8, MidiTarget::Master),
        (6, 13, MidiTarget::CueGain),
    ];
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        cases.extend([
            (ch, 0, MidiTarget::Tempo { deck }),
            (
                ch,
                7,
                MidiTarget::Eq {
                    deck,
                    band: EqBand::High,
                },
            ),
            (
                ch,
                11,
                MidiTarget::Eq {
                    deck,
                    band: EqBand::Mid,
                },
            ),
            (
                ch,
                15,
                MidiTarget::Eq {
                    deck,
                    band: EqBand::Low,
                },
            ),
            (ch, 19, MidiTarget::Fader { deck }),
            (6, 0x17 + ch, MidiTarget::Filter { deck }),
        ]);
    }
    for (ch, msb, target) in cases {
        let mut r = Router::default();
        assert!(route(&mut r, &p, &[0xb0 | ch, msb, 127]).is_empty());
        assert_eq!(
            route(&mut r, &p, &[0xb0 | ch, msb + 32, 127]),
            vec![MidiAction {
                target: target.clone(),
                value: MidiValue::Normalized(1.),
            }]
        );
        route(&mut r, &p, &[0xb0 | ch, msb, 0]);
        assert_eq!(
            route(&mut r, &p, &[0xb0 | ch, msb + 32, 0]),
            vec![MidiAction {
                target,
                value: MidiValue::Normalized(0.),
            }]
        );
    }
}

#[test]
fn flx2_jog_direction_touch_and_shift_release_follow_manufacturer_encoding() {
    let p = preset();
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        let mut r = Router::default();
        for number in [0x21, 0x22, 0x23, 0x29] {
            for (value, frames) in [(0x41, 110.25), (0x3f, -110.25)] {
                assert_eq!(
                    route(&mut r, &p, &[0xb0 | ch, number, value]),
                    vec![MidiAction {
                        target: MidiTarget::Jog { deck },
                        value: MidiValue::JogFrames(frames),
                    }]
                );
            }
            assert!(route(&mut r, &p, &[0xb0 | ch, number, 0x40]).is_empty());
        }
        assert_eq!(
            route(&mut r, &p, &[0x90 | ch, 0x36, 127])[0].value,
            MidiValue::Press
        );
        assert_eq!(
            route(&mut r, &p, &[0x90 | ch, 0x3f, 127]),
            vec![MidiAction {
                target: MidiTarget::JogTouch { deck },
                value: MidiValue::Release,
            }]
        );
        assert_eq!(
            route(&mut r, &p, &[0x90 | ch, 0x67, 127])[0].value,
            MidiValue::Press
        );
        assert_eq!(
            route(&mut r, &p, &[0x90 | ch, 0x3f, 0])[0].value,
            MidiValue::Release
        );
    }
}

#[test]
fn flx2_feedback_has_only_documented_leds_and_output_only_vinyl_setting() {
    let p = preset();
    let mut s = EngineSnapshot::default();
    s.decks[0].playing = true;
    s.decks[0].synced = true;
    s.decks[0].pfl = true;
    s.decks[0].cues[7] = Some(100);
    s.decks[0].vinyl = true;
    let bytes: Vec<_> = p
        .feedback
        .iter()
        .map(|f| feedback_packet(f, &s).unwrap().data)
        .collect();
    for packet in [
        vec![0x90, 0x0b, 127],
        vec![0x90, 0x58, 127],
        vec![0x90, 0x54, 127],
        vec![0x97, 7, 127],
        vec![0x90, 0x17, 127],
        vec![0x91, 0x17, 0],
    ] {
        assert!(
            bytes.contains(&packet),
            "missing documented output {packet:?}"
        );
    }
    for deck_ch in [0, 1] {
        assert!(
            p.shutdown
                .iter()
                .any(|packet| packet.data == [0x90 | deck_ch, 0x17, 127])
        );
    }
    assert!(route(&mut Router::default(), &p, &[0x90, 0x17, 127]).is_empty());
    assert!(bytes.iter().all(|packet| packet[0] & 0xf0 == 0x90));
}

#[test]
fn flx2_portable_preset_and_independent_replay_match_the_builtin() {
    let p = preset();
    let packaged: ControllerPreset =
        serde_json::from_str(include_str!("../../../../examples/ddj-flx2/preset.json")).unwrap();
    assert_eq!(p, packaged);
    let trace =
        serde_json::from_str(include_str!("../../../../examples/ddj-flx2/trace.json")).unwrap();
    let result = crate::replay::replay(&p, &trace, true).unwrap();
    assert!(result.assertions >= 30);
}
