//! Numark's DJ2GO Rev B communications specification, independently implemented.
use super::*;

pub(super) fn dj2go() -> ControllerPreset {
    let mut p = profile("DJ2GO", "Numark", "https://www.numark.com/images/product_downloads/dj2go___software_definition_and_midi_spec_v1.0.pdf", "Rev B", "pp. 5–7: buttons, LED feedback, absolute controls and signed encoder values", "Original DJ2GO only; DJ2GO2 and Touch are different devices. Transport, browse/load, volume, tempo, jog and LEDs. No touch-sensitive scratch, pads or built-in audio routing. Pitch-bend buttons and BACK are unmapped. Jog calibration and hardware testing pending.");
    for (deck, cue, play, sync, pfl, load, volume, pitch, wheel) in [
        (DeckId::A, 0x33, 0x3b, 0x40, 0x65, 0x4b, 8, 13, 0x19),
        (DeckId::B, 0x3c, 0x42, 0x47, 0x66, 0x34, 9, 14, 0x18),
    ] {
        for (number, target) in [
            (cue, MidiTarget::TemporaryCue { deck }),
            (play, MidiTarget::Play { deck }),
            (sync, MidiTarget::Sync { deck }),
            (pfl, MidiTarget::Pfl { deck }),
        ] {
            lit_note(&mut p, 0, number, target);
        }
        note(&mut p, 0, load, MidiTarget::LoadSelected { deck });
        cc(
            &mut p,
            0,
            volume,
            MidiTarget::Fader { deck },
            ValueMode::Absolute,
        );
        cc(
            &mut p,
            0,
            pitch,
            MidiTarget::Tempo { deck },
            ValueMode::Absolute,
        );
        cc(
            &mut p,
            0,
            wheel,
            MidiTarget::Jog { deck },
            ValueMode::TwosComplement,
        );
    }
    for (number, target) in [
        (0x0a, MidiTarget::Xfader),
        (0x17, MidiTarget::Master),
        (0x0b, MidiTarget::CueGain),
    ] {
        cc(&mut p, 0, number, target, ValueMode::Absolute);
    }
    cc(
        &mut p,
        0,
        0x1a,
        MidiTarget::BrowseTracks,
        ValueMode::TwosComplement,
    );
    note(&mut p, 0, 0x5a, MidiTarget::LoadFocused);
    p
}
