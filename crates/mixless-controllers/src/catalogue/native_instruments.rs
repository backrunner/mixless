//! Numeric MIDI addresses from named NI Controller Editor templates.
//! Assignments below are independently chosen for mixless, not factory mappings.
use super::*;
use mixless_protocol::StemKind;

const SOURCE: &str = "https://support.native-instruments.com/support/solutions/articles/69000882238-native-instruments-drivers-other-files";

pub(super) fn profiles() -> Vec<ControllerPreset> {
    vec![f1(), x1()]
}
fn template(model: &str, label: &str, filename: &str, notes: &str) -> ControllerPreset {
    let mut p = profile(
        &format!("{model} ({label})"),
        "Native Instruments",
        SOURCE,
        "Controller Editor 2.8.2",
        &format!(
            "Templates/{filename}; Basic page; see NI_COMPATIBILITY.md for source fingerprints"
        ),
        notes,
    );
    p.id = if model.contains("F1") {
        "builtin-ni-f1-fl-studio-v1"
    } else {
        "builtin-ni-x1-ssl22-v1"
    }
    .into();
    p.endpoints[0].matcher = DeviceMatcher::Midi {
        names: vec![model.into()],
    };
    p
}
fn f1() -> ControllerPreset {
    let mut p = template("Traktor Kontrol F1", "FL Studio template", "FL Studio.nckf1",
        "Requires Controller Editor 2.8.2's unmodified FL Studio template, Basic page, and MIDI mode (Shift + Browse). This is an independent mixless transport/stem layout, not a factory MIDI mapping. Pads 1–8 control deck A and 9–16 deck B: Play, Sync, PFL, Loop, Key Lock, Vinyl, Slip, Reverse. Knobs control A stems; faders control B stems. Shift page, display and pad colors are unmapped. Pad brightness feedback only. Hardware unverified.");
    for (i, stem) in [
        StemKind::Vocals,
        StemKind::Drums,
        StemKind::Bass,
        StemKind::Instruments,
    ]
    .into_iter()
    .enumerate()
    {
        cc(
            &mut p,
            0,
            i as u8,
            MidiTarget::Stem {
                deck: DeckId::A,
                stem,
            },
            ValueMode::Absolute,
        );
        cc(
            &mut p,
            1,
            i as u8,
            MidiTarget::Stem {
                deck: DeckId::B,
                stem,
            },
            ValueMode::Absolute,
        );
    }
    for (offset, deck) in [(0, DeckId::A), (8, DeckId::B)] {
        for (i, target) in [
            MidiTarget::Play { deck },
            MidiTarget::Sync { deck },
            MidiTarget::Pfl { deck },
            MidiTarget::Loop { deck },
            MidiTarget::KeyLock { deck },
            MidiTarget::Vinyl { deck },
            MidiTarget::Slip { deck },
            MidiTarget::Reverse { deck },
        ]
        .into_iter()
        .enumerate()
        {
            let number = offset + i as u8;
            note(&mut p, 15, number, target.clone());
            let packet = OutputPacket {
                endpoint: "main".into(),
                data: vec![0xbb, number, 0],
                report: None,
            };
            p.shutdown.push(packet.clone());
            p.feedback.push(FeedbackRule {
                signal: FeedbackSignal::Control { target },
                packet,
                bit_offset: 16,
                bits: 7,
                little_endian: true,
                off: 0,
                on: 127,
            });
        }
    }
    cc(
        &mut p,
        12,
        127,
        MidiTarget::BrowseTracks,
        ValueMode::BinaryOffset,
    );
    note(&mut p, 0, 0, MidiTarget::LoadFocused);
    for (number, target) in [
        (16, MidiTarget::LoadSelected { deck: DeckId::A }),
        (18, MidiTarget::LoadSelected { deck: DeckId::B }),
        (33, MidiTarget::Quantize),
    ] {
        note(&mut p, 14, number, target);
    }
    p
}
fn x1() -> ControllerPreset {
    let mut p = template("Traktor Kontrol X1 MK1", "SSL 2.2 template", "Serato Scratch Live 2.2.nckx1",
        "Original X1 MK1 only. Requires Controller Editor 2.8.2's unmodified Serato Scratch Live 2.2 template, Basic page, and MIDI mode (Shift + Hotcue). Independent mixless assignments: dry/wet knobs control FX mix, P1–P3 control EQ, Cue buttons control PFL, Play/Sync control transport, upper pushes load, lower pushes loop, beat buttons jump. Both browse wheels send the same address. Loop-size wheels, Shift page, temporary cue, lights and display are unmapped. Hardware unverified.");
    p.endpoints[0].matcher = DeviceMatcher::Midi {
        names: vec!["Traktor Kontrol X1".into(), "Traktor Kontrol X1 MK1".into()],
    };
    for (deck, side) in [(DeckId::A, 0), (DeckId::B, 1)] {
        cc(
            &mut p,
            0,
            side,
            MidiTarget::FxMix { deck, slot: 0 },
            ValueMode::Absolute,
        );
        for (offset, band) in [(2, EqBand::High), (4, EqBand::Mid), (6, EqBand::Low)] {
            cc(
                &mut p,
                0,
                offset + side,
                MidiTarget::Eq { deck, band },
                ValueMode::Absolute,
            );
        }
        for (offset, target) in [
            (8, MidiTarget::FxOn { deck, slot: 0 }),
            (18, MidiTarget::LoadSelected { deck }),
            (24, MidiTarget::Loop { deck }),
            (32, MidiTarget::BeatBack { deck }),
            (34, MidiTarget::BeatForward { deck }),
            (36, MidiTarget::Pfl { deck }),
            (40, MidiTarget::Play { deck }),
            (42, MidiTarget::Sync { deck }),
        ] {
            cc(&mut p, 0, offset + side, target, ValueMode::Auto);
        }
    }
    cc(
        &mut p,
        0,
        16,
        MidiTarget::BrowseTracks,
        ValueMode::TwosComplement,
    );
    p
}
