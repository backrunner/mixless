//! Independently read Hercules MIDI command tables; no third-party mapping imports.
use super::*;

pub(super) fn profiles() -> Vec<ControllerPreset> {
    [200, 300].into_iter().map(inpulse).collect()
}
fn inpulse(model: u16) -> ControllerPreset {
    let name = format!("DJControl Inpulse {model}");
    let revision = if model == 200 { "1.0" } else { "1.2" };
    let file_revision = revision.replace('.', "_");
    let url = format!("https://ts.hercules.com/download/sound/manuals/DJC_Inpulse{model}/DJControlInpulse{model}_MIDI_Commands_V_{file_revision}.pdf");
    let section = if model == 200 {
        "PDF pp. 4–19: inputs; pp. 20–23: LEDs; p. 29: status request"
    } else {
        "PDF pp. 4–39: inputs; pp. 40–54: LEDs"
    };
    let mut p = profile(&name, "Hercules", &url, revision, section, "Original model only; MK2/MK3 are not covered. Use HOT CUE mode; Shift + pad clears a cue. LOOP OUT toggles mixless's current loop. Beatmatch guide, FX and other pad modes are unmapped. Shifted deck buttons are omitted where the official table conflicts. Jog calibration and hardware testing pending.");
    cc14(&mut p, 0, 0, MidiTarget::Xfader);
    cc14(&mut p, 0, 3, MidiTarget::Master);
    cc14(&mut p, 0, 4, MidiTarget::CueGain);
    cc(
        &mut p,
        0,
        1,
        MidiTarget::BrowseTracks,
        ValueMode::TwosComplement,
    );
    cc(
        &mut p,
        3,
        1,
        MidiTarget::BrowsePlaylists,
        ValueMode::TwosComplement,
    );
    note(&mut p, 0, 0, MidiTarget::LoadFocused);
    for (ch, deck) in [(1, DeckId::A), (2, DeckId::B)] {
        for (number, target) in [
            (3, MidiTarget::Vinyl { deck }),
            (5, MidiTarget::Sync { deck }),
            (6, MidiTarget::TemporaryCue { deck }),
            (7, MidiTarget::Play { deck }),
            (10, MidiTarget::Loop { deck }),
            (12, MidiTarget::Pfl { deck }),
        ] {
            lit_note(&mut p, ch, number, target);
        }
        note(&mut p, ch, 8, MidiTarget::JogTouch { deck });
        note(&mut p, ch, 13, MidiTarget::LoadSelected { deck });
        for layer_ch in [ch, ch + 3] {
            for (number, target) in [
                (0, MidiTarget::Fader { deck }),
                (1, MidiTarget::Filter { deck }),
                (
                    2,
                    MidiTarget::Eq {
                        deck,
                        band: EqBand::Low,
                    },
                ),
                (
                    4,
                    MidiTarget::Eq {
                        deck,
                        band: EqBand::High,
                    },
                ),
                (8, MidiTarget::Tempo { deck }),
            ] {
                cc14(&mut p, layer_ch, number, target);
            }
            if model == 300 {
                cc14(
                    &mut p,
                    layer_ch,
                    3,
                    MidiTarget::Eq {
                        deck,
                        band: EqBand::Mid,
                    },
                );
                cc14(&mut p, layer_ch, 5, MidiTarget::Gain { deck });
            }
            for number in [9, 10] {
                cc(
                    &mut p,
                    layer_ch,
                    number,
                    MidiTarget::Jog { deck },
                    ValueMode::TwosComplement,
                );
            }
        }
        let count = if model == 200 { 4 } else { 8 };
        for index in 0..count {
            lit_note(&mut p, ch + 5, index, MidiTarget::Cue { deck, index });
            note(
                &mut p,
                ch + 5,
                index + 8,
                MidiTarget::ClearCue { deck, index },
            );
        }
        let layer = format!("shift-{ch}");
        guard_deck_notes(&mut p, deck, &layer, None);
        modifier(&mut p, ch, 4, &layer);
    }
    if model == 200 {
        // Do not extrapolate this request to models whose message table omits it.
        p.initialize.push(packet([0xb0, 0x7f, 0x7f]));
    }
    p
}
