//! Model-specific DDJ inputs independently checked against manufacturer PDFs.
//! The retained review/trace fixtures record the source and exclusions per model.
use super::*;
mod models;
#[cfg(test)]
mod tests;

struct Model {
    name: &'static str,
    file: &'static str,
    pages: &'static str,
    jog: &'static [u8],
    touch: bool,
    shared_shift: Option<u8>,
    sync: u8,
    cue: bool,
    pfl: Option<u8>,
    mixer: bool,
    modern_pads: bool,
    pad_notes: &'static [u8],
    pad_on: u32,
    loop_note: Option<u8>,
    loop_sizes: Option<(u8, u8)>,
    key_lock: Option<u8>,
    volumes: &'static [u8],
    browse_push: bool,
    connect: Option<u8>,
    disconnect: Option<u8>,
    notes: &'static str,
}

pub(super) fn profiles() -> Vec<ControllerPreset> {
    let mut profiles: Vec<_> = models::MODELS.iter().map(build).collect();
    profiles.extend([wego(2), wego(4)]);
    profiles
}

fn build(m: &Model) -> ControllerPreset {
    let mut p = pioneer(
        m.name,
        m.file,
        "Manufacturer MIDI list",
        m.pages,
        "Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending.",
    );
    p.notes.push(' ');
    p.notes.push_str(m.notes);
    if m.browse_push && m.shared_shift.is_none() {
        p.notes
            .push_str(" Browser push is unmapped; use the deck LOAD buttons.");
    }
    if m.name == "DDJ-XP2" {
        p.notes = "Decks 1/2 only. Select PAD MODE 1; Shift + pad clears a cue. ".into();
        p.notes.push_str(m.notes);
        p.notes.push_str(" Hardware testing pending.");
    }
    if m.name == "DDJ-1000SRT" {
        p.provenance[0].url = format!(
            "https://downloads.support.alphatheta.com/firmwares/dj-controllers/{}/{}",
            m.name, m.file
        );
    }
    if m.name == "DDJ-FLX6-GT" {
        p.provenance[0].url = "https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-FLX6/DDJ-FLX6_MIDI_message_List_E1.pdf".into();
        p.provenance.push(Provenance {
            url: "https://support.alphatheta.com/en-us/articles/4404631697945".into(),
            revision: "FLX6/FLX6-GT compatibility article".into(),
            section: "Shared MIDI message list; utility setting for other DJ software".into(),
        });
    }
    let accessory = !m.mixer;
    if let Some(value) = m.connect {
        p.initialize.push(packet([0x9b, 9, value]));
    }
    // A global Shift packet can retire the browser's button edge. For devices
    // with separate deck Shifts, encoder push is deliberately left unmapped.
    browse(
        &mut p,
        (m.browse_push && m.shared_shift.is_some()).then_some(6),
    );
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        if !accessory {
            lit_note(&mut p, ch, 0x0b, MidiTarget::Play { deck });
            if m.cue {
                lit_note(&mut p, ch, 0x0c, MidiTarget::TemporaryCue { deck });
            }
            for &number in m.jog {
                cc(
                    &mut p,
                    ch,
                    number,
                    MidiTarget::Jog { deck },
                    ValueMode::BinaryOffset,
                );
            }
            if m.touch {
                note(&mut p, ch, 0x36, MidiTarget::JogTouch { deck });
                note(&mut p, ch, 0x67, MidiTarget::JogTouch { deck });
            }
            cc14(&mut p, ch, 0, MidiTarget::Tempo { deck });
            for (msb, target) in [
                (4, MidiTarget::Gain { deck }),
                (
                    7,
                    MidiTarget::Eq {
                        deck,
                        band: EqBand::High,
                    },
                ),
                (
                    11,
                    MidiTarget::Eq {
                        deck,
                        band: EqBand::Mid,
                    },
                ),
                (
                    15,
                    MidiTarget::Eq {
                        deck,
                        band: EqBand::Low,
                    },
                ),
                (19, MidiTarget::Fader { deck }),
            ] {
                cc14(&mut p, ch, msb, target);
            }
            cc14(&mut p, 6, 0x17 + ch, MidiTarget::Filter { deck });
            if let Some(number) = m.pfl {
                lit_note(&mut p, ch, number, MidiTarget::Pfl { deck });
            }
        }
        lit_note(&mut p, ch, m.sync, MidiTarget::Sync { deck });
        if let Some(number) = m.key_lock {
            lit_note(&mut p, ch, number, MidiTarget::KeyLock { deck });
        }
        // GRV6's RELOOP/EXIT has no documented output, so loop inputs have no
        // inferred LED. All bindings operate on mixless's current loop size.
        if let Some(number) = m.loop_note {
            note(&mut p, ch, number, MidiTarget::Loop { deck });
        }
        if let Some((half, double)) = m.loop_sizes {
            note(&mut p, ch, half, MidiTarget::LoopHalve { deck });
            note(&mut p, ch, double, MidiTarget::LoopDouble { deck });
        }
        let pad_ch = 7 + if m.modern_pads { ch * 2 } else { ch };
        let shift_ch = pad_ch + u8::from(m.modern_pads);
        for (index, &number) in m.pad_notes.iter().enumerate() {
            let index = index as u8;
            lit_note(&mut p, pad_ch, number, MidiTarget::Cue { deck, index });
            // Binary LEDs use 127; colour LEDs use an explicitly documented
            // palette entry. Never infer a colour range from another model.
            p.feedback.last_mut().unwrap().on = m.pad_on;
            note(
                &mut p,
                shift_ch,
                number + if m.modern_pads { 0 } else { 8 },
                MidiTarget::ClearCue { deck, index },
            );
        }
        let layer = if m.shared_shift.is_some() {
            "shift".into()
        } else {
            format!("shift-{ch}")
        };
        guard_deck_notes(&mut p, deck, &layer, Some(0x67));
        if m.shared_shift.is_none() {
            modifier(&mut p, ch, 0x3f, &layer);
        }
    }
    if let Some(number) = m.shared_shift {
        modifier(&mut p, 6, number, "shift");
    }
    if m.mixer {
        cc14(&mut p, 6, 0x1f, MidiTarget::Xfader);
    }
    for &msb in m.volumes {
        let target = match msb {
            8 => MidiTarget::Master,
            12 => MidiTarget::CueMix,
            13 => MidiTarget::CueGain,
            _ => unreachable!(),
        };
        cc14(&mut p, 6, msb, target);
    }
    if m.shared_shift.is_some() {
        for b in &mut p.bindings {
            if b.target.deck().is_none()
                && b.sets_layer.is_none()
                && matches!(
                    b.source,
                    InputSource::Midi {
                        address: MidiAddress::Note { .. }
                    }
                )
            {
                b.layer = Some(LayerCondition {
                    name: "shift".into(),
                    active: false,
                });
            }
        }
    }
    if let Some(value) = m.disconnect {
        p.shutdown.push(packet([0x9b, 9, value]));
    }
    p
}

fn wego(generation: u8) -> ControllerPreset {
    let (model, file, pages) = if generation == 2 {
        (
            "DDJ-WeGO2",
            "DDJWeGO2ListofMIDIMessagealgoriddimE.pdf",
            "pp. 2–3: deck, mixer, browser, Shift and outputs",
        )
    } else {
        (
            "DDJ-WeGO4",
            "DDJ-WeGO4_List_of_MIDI_Message_E.pdf",
            "pp. 1–2: deck, mixer, browser, Shift and outputs",
        )
    };
    let mut p = pioneer(
        model,
        file,
        "Manufacturer MIDI list",
        pages,
        "Two decks, four hot cues per deck. Select HOT CUE mode; Shift + cue clears it. Hardware controls master/headphone volume. Other pad modes, effects and fader start are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending.",
    );
    browse(&mut p, Some(6));
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        transport(&mut p, ch, deck, &[0x21, 0x22, 0x26, 0x27]);
        // WeGO2 has a loop encoder, not separate halve/double buttons.
        note(&mut p, ch, 0x14, MidiTarget::Loop { deck });
        if generation == 4 {
            note(&mut p, ch, 0x12, MidiTarget::LoopHalve { deck });
            note(&mut p, ch, 0x13, MidiTarget::LoopDouble { deck });
        }
        for index in 0..4 {
            lit_note(&mut p, ch, 0x2e + index, MidiTarget::Cue { deck, index });
            note(
                &mut p,
                ch,
                0x5f + index,
                MidiTarget::ClearCue { deck, index },
            );
        }
        for (number, band) in [(7, EqBand::High), (11, EqBand::Mid), (15, EqBand::Low)] {
            if generation == 4 && band == EqBand::Mid {
                continue;
            }
            cc14(&mut p, 6, number + ch, MidiTarget::Eq { deck, band });
        }
        if generation == 4 {
            cc14(&mut p, 6, 0x17 + ch, MidiTarget::Filter { deck });
        }
        cc14(&mut p, 6, 0x13 + ch * 2, MidiTarget::Fader { deck });
        lit_note(&mut p, 6, 0x54 + ch, MidiTarget::Pfl { deck });
        guard_deck_notes(&mut p, deck, "shift", Some(0x67));
    }
    modifier(&mut p, 6, 0x4f, "shift");
    cc14(&mut p, 6, 0x1f, MidiTarget::Xfader);
    if generation == 2 {
        cc14(&mut p, 6, 1, MidiTarget::CueMix);
        p.notes.push_str(" The loop-size encoder is unmapped.");
    }
    for b in &mut p.bindings {
        if b.target == MidiTarget::LoadFocused {
            b.layer = Some(LayerCondition {
                name: "shift".into(),
                active: false,
            });
        }
    }
    p
}
