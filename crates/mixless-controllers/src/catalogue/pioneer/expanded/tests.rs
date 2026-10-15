use super::*;
use crate::{replay::feedback_packet, router::Router};
use mixless_protocol::EngineSnapshot;

fn preset(name: &str) -> ControllerPreset {
    profiles().into_iter().find(|p| p.name == name).unwrap()
}
fn route(r: &mut Router, p: &ControllerPreset, bytes: &[u8]) -> Vec<MidiAction> {
    r.route("unit", "main", p, false, bytes, &EngineSnapshot::default())
}

#[test]
fn rev5_uses_its_own_sync_pfl_loop_and_shift_messages() {
    let p = preset("DDJ-REV5");
    let mut r = Router::default();
    for (number, target) in [
        (10, MidiTarget::Sync { deck: DeckId::A }),
        (7, MidiTarget::Pfl { deck: DeckId::A }),
        (4, MidiTarget::Loop { deck: DeckId::A }),
        (1, MidiTarget::LoopHalve { deck: DeckId::A }),
        (3, MidiTarget::LoopDouble { deck: DeckId::A }),
    ] {
        assert_eq!(
            route(&mut r, &p, &[0x90, number, 127]),
            vec![MidiAction {
                target,
                value: MidiValue::Press
            }]
        );
    }
    for number in [0x58, 0x54, 0x14, 0x36, 0x67, 0x0d] {
        assert!(route(&mut r, &p, &[0x90, number, 127]).is_empty());
    }
    route(&mut r, &p, &[0x96, 0x3f, 127]);
    assert!(route(&mut r, &p, &[0x90, 10, 127]).is_empty());
    assert_eq!(
        route(&mut r, &p, &[0x9a, 7, 127])[0].target,
        MidiTarget::ClearCue {
            deck: DeckId::B,
            index: 7
        }
    );
}

#[test]
fn xp2_maps_physical_pad_order_and_ignores_other_pages() {
    let p = preset("DDJ-XP2");
    let mut r = Router::default();
    for (index, number) in [12, 13, 14, 15, 8, 9, 10, 11].into_iter().enumerate() {
        assert_eq!(
            route(&mut r, &p, &[0x99, number, 127])[0].target,
            MidiTarget::Cue {
                deck: DeckId::B,
                index: index as u8
            }
        );
    }
    for number in [0, 1, 4, 7, 0x1c, 0x3c, 0x7c] {
        assert!(route(&mut r, &p, &[0x97, number, 127]).is_empty());
    }
    route(&mut r, &p, &[0x96, 0x40, 127]);
    assert_eq!(
        route(&mut r, &p, &[0x98, 12, 127])[0].target,
        MidiTarget::ClearCue {
            deck: DeckId::A,
            index: 0
        }
    );
    assert!(route(&mut r, &p, &[0x98, 0, 127]).is_empty());
}

#[test]
fn wego_generations_keep_mixer_channels_and_two_band_eq_distinct() {
    for name in ["DDJ-WeGO2", "DDJ-WeGO4"] {
        let p = preset(name);
        let mut r = Router::default();
        assert!(route(&mut r, &p, &[0xb6, 0x15, 127]).is_empty());
        assert_eq!(
            route(&mut r, &p, &[0xb6, 0x35, 127]),
            vec![MidiAction {
                target: MidiTarget::Fader { deck: DeckId::B },
                value: MidiValue::Normalized(1.)
            }]
        );
        route(&mut r, &p, &[0xb1, 0x13, 127]);
        assert!(route(&mut r, &p, &[0xb1, 0x33, 127]).is_empty());
        assert_eq!(
            route(&mut r, &p, &[0x96, 0x55, 127])[0].target,
            MidiTarget::Pfl { deck: DeckId::B }
        );
        assert!(
            !p.bindings
                .iter()
                .any(|b| b.target == MidiTarget::Master && b.sets_layer.is_none())
        );
    }
    let p = preset("DDJ-WeGO4");
    assert!(!p.bindings.iter().any(|b| matches!(
        b.target,
        MidiTarget::Eq {
            band: EqBand::Mid,
            ..
        }
    )));
    let p = preset("DDJ-WeGO2");
    let mut r = Router::default();
    for number in [0x12, 0x13] {
        assert!(route(&mut r, &p, &[0x90, number, 127]).is_empty());
    }
    route(&mut r, &p, &[0xb6, 1, 127]);
    assert_eq!(
        route(&mut r, &p, &[0xb6, 33, 127])[0].target,
        MidiTarget::CueMix
    );
}

#[test]
fn motor_controller_and_pad_accessories_do_not_invent_missing_controls() {
    let p = preset("DDJ-REV7");
    assert!(!p.bindings.iter().any(|b| matches!(
        b.target,
        MidiTarget::TemporaryCue { .. } | MidiTarget::JogTouch { .. } | MidiTarget::Pfl { .. }
    )));
    for name in ["DDJ-SP1", "DDJ-XP1", "DDJ-XP2"] {
        let p = preset(name);
        assert!(!p.bindings.iter().any(|b| matches!(
            b.target,
            MidiTarget::Play { .. }
                | MidiTarget::Tempo { .. }
                | MidiTarget::Jog { .. }
                | MidiTarget::Fader { .. }
                | MidiTarget::Xfader
        )));
        assert!(p.audio.is_none());
    }
    for name in [
        "DDJ-WeGO",
        "DDJ-T1",
        "DDJ-S1",
        "DDJ-ERGO",
        "CDJ-3000",
        "DJM-900NXS2",
    ] {
        assert!(!profiles().iter().any(|p| p.matches_midi(name)));
    }
}

#[test]
fn per_deck_and_shared_shift_retire_holds_without_cross_device_or_deck_leaks() {
    for (name, status, number) in [
        ("DDJ-1000", 0x90, 0x3f),
        ("DDJ-SX3", 0x90, 0x3f),
        ("DDJ-WeGO4", 0x96, 0x4f),
    ] {
        let p = preset(name);
        let mut r = Router::default();
        route(&mut r, &p, &[0x90, 0x0c, 127]);
        route(&mut r, &p, &[0x91, 0x0c, 127]);
        let releases = route(&mut r, &p, &[status, number, 127]);
        assert!(releases.contains(&MidiAction {
            target: MidiTarget::TemporaryCue { deck: DeckId::A },
            value: MidiValue::Release
        }));
        assert_eq!(
            releases
                .iter()
                .any(|a| a.target == MidiTarget::TemporaryCue { deck: DeckId::B }),
            name == "DDJ-WeGO4"
        );
        route(&mut r, &p, &[status, number, 0]);
        assert_eq!(
            route(&mut r, &p, &[0x90, 0x0c, 127])[0].value,
            MidiValue::Hold
        );
    }
}

#[test]
fn pad_output_values_fit_documented_binary_brightness_and_colour_ranges() {
    let mut state = EngineSnapshot::default();
    state.decks[0].cues[0] = Some(42);
    for (name, packet) in [
        ("DDJ-RR", vec![0x97, 0, 1]),
        ("DDJ-FLX6", vec![0x97, 0, 127]),
        ("DDJ-SP1", vec![0x97, 0, 127]),
        ("DDJ-XP2", vec![0x97, 12, 127]),
    ] {
        let p = preset(name);
        assert!(
            p.feedback
                .iter()
                .filter_map(|f| feedback_packet(f, &state))
                .any(|p| p.data == packet),
            "{name}"
        );
    }
}

#[test]
fn reviewed_model_replays_remain_pinned_and_fully_asserted() {
    let traces: Vec<crate::replay::ReportTrace> = serde_json::from_str(include_str!(
        "../../../../examples/alphatheta-ddj/replays.json"
    ))
    .unwrap();
    assert_eq!(traces.len(), 24);
    let profiles = profiles();
    for p in profiles {
        assert_eq!(p.verification, Verification::Protocol);
        assert!(p.audio.is_none());
        let trace = traces.iter().find(|t| t.model == p.name).unwrap();
        let result =
            crate::replay::replay(&p, trace, true).unwrap_or_else(|e| panic!("{}: {e}", p.name));
        assert_eq!(result.frames, result.assertions);
        assert!(result.assertions > 40, "{}", p.name);
    }
}
