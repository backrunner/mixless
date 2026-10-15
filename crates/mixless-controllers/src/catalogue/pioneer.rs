//! Addresses independently read from AlphaTheta message lists; see SOURCES.md.
use super::*;
mod expanded;
mod flx2;

pub(super) fn rekordbox_entry() -> Vec<ControllerPreset> {
    [false,true].into_iter().map(|flx|{
        let model=if flx {"DDJ-FLX4"} else {"DDJ-400"};
        let mut p=ControllerPreset::blank(model);
        p.id=format!("builtin-{}-v1",model.to_lowercase());
        p.manufacturer="Pioneer DJ / AlphaTheta".into();
        p.notes="Two decks. HOT CUE mode; Shift + pad clears a cue. Other pad banks, Smart FX and fader start are unmapped. Jog calibration and hardware testing pending.".into();
        p.endpoints[0].matcher=DeviceMatcher::Midi{names:vec![model.into()]};
        let file=if flx {"DDJ-FLX4_MIDI_message_List_E1.pdf"}else{"DDJ-400_MIDI_Message_List_E1.pdf"};
        p.provenance.push(Provenance{url:format!("https://downloads.support.alphatheta.com/software_info/dj-controllers/{model}/{file}"),revision:if flx {"1.0"} else {"1.00"}.into(),section:if flx {"pp. 1–5: deck, mixer, browse, hot cue, vinyl output"}else{"pp. 1–3: deck, mixer, browse, hot cue, vinyl output"}.into()});
        p.audio=Some(AudioRecommendation{device_name:model.into(),master:[0,1],cue:[2,3]});
        // App semantics are deliberately explicit: normal pads trigger cues, shifted pads clear.
        for (ch,deck) in [(0,DeckId::A),(1,DeckId::B)] {
            note(&mut p,ch,0x0b,MidiTarget::Play{deck});
            note(&mut p,ch,0x0c,MidiTarget::TemporaryCue{deck});
            note(&mut p,ch,0x58,MidiTarget::Sync{deck});
            note(&mut p,ch,0x54,MidiTarget::Pfl{deck});
            note(&mut p,ch,0x4d,MidiTarget::Loop{deck});
            note(&mut p,ch,0x51,MidiTarget::BeatBack{deck});
            note(&mut p,ch,0x53,MidiTarget::BeatForward{deck});
            note(&mut p,ch,0x36,MidiTarget::JogTouch{deck});
            note(&mut p,ch,0x67,MidiTarget::JogTouch{deck});
            for number in [0x21,0x22,0x23,0x29] { input(&mut p,MidiAddress::Cc{channel:ch,number},MidiTarget::Jog{deck},ValueMode::BinaryOffset); }
            for (msb,target) in [(0,MidiTarget::Tempo{deck}),(4,MidiTarget::Gain{deck}),(7,MidiTarget::Eq{deck,band:EqBand::High}),(11,MidiTarget::Eq{deck,band:EqBand::Mid}),(15,MidiTarget::Eq{deck,band:EqBand::Low}),(19,MidiTarget::Fader{deck})] {cc14(&mut p,ch,msb,target);}
            cc14(&mut p,6,0x17+ch,MidiTarget::Filter{deck});
            for index in 0..8 {
                note(&mut p,7+ch*2,index,MidiTarget::Cue{deck,index});
                note(&mut p,8+ch*2,index,MidiTarget::ClearCue{deck,index});
                led(&mut p,7+ch*2,index,MidiTarget::Cue{deck,index});
            }
            for (number,target) in [(0x0b,MidiTarget::Play{deck}),(0x0c,MidiTarget::TemporaryCue{deck}),(0x58,MidiTarget::Sync{deck}),(0x54,MidiTarget::Pfl{deck}),(0x17,MidiTarget::Vinyl{deck})] {led(&mut p,ch,number,target);}
            let packet=OutputPacket{endpoint:"main".into(),data:vec![0xb0|ch,2,0],report:None};
            p.shutdown.push(packet.clone());
            p.feedback.push(FeedbackRule{signal:FeedbackSignal::Meter{deck:Some(deck)},packet,bit_offset:16,bits:7,little_endian:true,off:0,on:127});
        }
        cc14(&mut p,6,0x1f,MidiTarget::Xfader);
        cc14(&mut p,6,8,MidiTarget::Master);
        cc14(&mut p,6,13,MidiTarget::CueGain);
        input(&mut p,MidiAddress::Cc{channel:6,number:0x40},MidiTarget::BrowseTracks,ValueMode::TwosComplement);
        input(&mut p,MidiAddress::Cc{channel:6,number:0x64},MidiTarget::BrowsePlaylists,ValueMode::TwosComplement);
        note(&mut p,6,0x41,MidiTarget::LoadFocused);
        note(&mut p,6,0x46,MidiTarget::LoadSelected{deck:DeckId::A});
        note(&mut p,6,0x47,MidiTarget::LoadSelected{deck:DeckId::B});
        protect_shift(&mut p);
        p
    }).collect()
}

fn pioneer(
    model: &str,
    file: &str,
    revision: &str,
    section: &str,
    notes: &str,
) -> ControllerPreset {
    profile(
        model,
        "Pioneer DJ / AlphaTheta",
        &format!(
            "https://downloads.support.alphatheta.com/software_info/dj-controllers/{model}/{file}"
        ),
        revision,
        section,
        notes,
    )
}

// These addresses were checked separately in each model's manufacturer table.
// WeGO's mixer and SB's pad channels deliberately use different builders.
fn transport(p: &mut ControllerPreset, ch: u8, deck: DeckId, jog: &[u8]) {
    for (number, target) in [
        (0x0b, MidiTarget::Play { deck }),
        (0x0c, MidiTarget::TemporaryCue { deck }),
        (0x58, MidiTarget::Sync { deck }),
    ] {
        lit_note(p, ch, number, target);
    }
    for number in [0x36, 0x67] {
        note(p, ch, number, MidiTarget::JogTouch { deck });
    }
    for &number in jog {
        cc(
            p,
            ch,
            number,
            MidiTarget::Jog { deck },
            ValueMode::BinaryOffset,
        );
    }
    cc14(p, ch, 0, MidiTarget::Tempo { deck });
}
fn mixer(p: &mut ControllerPreset, ch: u8, deck: DeckId, trim: bool) {
    for (number, target) in [
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
        cc14(p, ch, number, target);
    }
    if trim {
        cc14(p, ch, 4, MidiTarget::Gain { deck });
    }
    cc14(p, 6, 0x17 + ch, MidiTarget::Filter { deck });
    lit_note(p, ch, 0x54, MidiTarget::Pfl { deck });
}
fn browse(p: &mut ControllerPreset, press_channel: Option<u8>) {
    cc(
        p,
        6,
        0x40,
        MidiTarget::BrowseTracks,
        ValueMode::TwosComplement,
    );
    cc(
        p,
        6,
        0x64,
        MidiTarget::BrowsePlaylists,
        ValueMode::TwosComplement,
    );
    if let Some(ch) = press_channel {
        note(p, ch, 0x41, MidiTarget::LoadFocused);
    }
    note(p, 6, 0x46, MidiTarget::LoadSelected { deck: DeckId::A });
    note(p, 6, 0x47, MidiTarget::LoadSelected { deck: DeckId::B });
}
fn pads(
    p: &mut ControllerPreset,
    ch: u8,
    shifted_ch: u8,
    shifted_base: u8,
    count: u8,
    deck: DeckId,
) {
    for index in 0..count {
        lit_note(p, ch, index, MidiTarget::Cue { deck, index });
        note(
            p,
            shifted_ch,
            shifted_base + index,
            MidiTarget::ClearCue { deck, index },
        );
    }
}
fn loops(p: &mut ControllerPreset, ch: u8, deck: DeckId) {
    lit_note(p, ch, 0x14, MidiTarget::Loop { deck });
    note(p, ch, 0x12, MidiTarget::LoopHalve { deck });
    note(p, ch, 0x13, MidiTarget::LoopDouble { deck });
}
fn software_volume(p: &mut ControllerPreset) {
    cc14(p, 6, 8, MidiTarget::Master);
    cc14(p, 6, 13, MidiTarget::CueGain);
}

pub(super) fn additional() -> Vec<ControllerPreset> {
    let mut presets = vec![
        ddj200(),
        flx2::profile(),
        sb(1),
        sb(2),
        sb(3),
        rb(),
        rev1(),
        wego3(),
    ];
    for p in &mut presets {
        protect_shift(p);
    }
    presets.extend(expanded::profiles());
    presets
}
fn ddj200() -> ControllerPreset {
    let mut p = pioneer(
        "DDJ-200",
        "DDJ-200_MIDI_Message_List_E2.pdf",
        "E2 (2019)",
        "p. 2: deck, mixer, pads and vinyl output",
        "Two decks, eight hot cues per deck; Shift + pad clears a cue. Use a separate audio device. Transition FX is unmapped. Jog calibration and hardware testing pending.",
    );
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        transport(&mut p, ch, deck, &[0x21, 0x22, 0x23, 0x29]);
        mixer(&mut p, ch, deck, false);
        pads(&mut p, 7 + ch * 2, 8 + ch * 2, 0, 8, deck);
        // Unlike an illuminated VINYL button, this output changes firmware jog mode.
        led(&mut p, ch, 0x17, MidiTarget::Vinyl { deck });
    }
    cc14(&mut p, 6, 0x1f, MidiTarget::Xfader);
    p
}
fn sb(generation: u8) -> ControllerPreset {
    let (model, file, revision, section) = match generation {
        1 => (
            "DDJ-SB",
            "DDJ_SB_List_of_MIDI_Messages_E.pdf",
            "1.00 (2013)",
            "pp. 1–2: deck, mixer, browse, pads and DJ App Connect",
        ),
        2 => (
            "DDJ-SB2",
            "DDJ-SB2_List_of_MIDI_Messages_E.pdf",
            "1.00 (2015)",
            "pp. 1–3: deck, mixer, browse, pads, meter and DJ App Connect",
        ),
        _ => (
            "DDJ-SB3",
            "ddj-sb3_midi_message_list_e2.pdf",
            "1.01 (2020)",
            "pp. 1–3: deck, mixer, browse, pads and meter",
        ),
    };
    let mut p = pioneer(
        model,
        file,
        revision,
        section,
        "Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, Pad Scratch, Filter Fade and fader start are unmapped. Jog calibration and hardware testing pending.",
    );
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        // SB3's table does not declare SB/SB2's shifted platter CC 1F.
        let jog: &[u8] = if generation == 3 {
            &[0x21, 0x22, 0x23, 0x26]
        } else {
            &[0x21, 0x22, 0x23, 0x1f, 0x26]
        };
        transport(&mut p, ch, deck, jog);
        mixer(&mut p, ch, deck, generation > 1);
        lit_note(&mut p, ch, 0x17, MidiTarget::Vinyl { deck });
        lit_note(&mut p, ch, 0x1a, MidiTarget::KeyLock { deck });
        let count = if generation == 3 { 8 } else { 4 };
        pads(&mut p, 7 + ch, 7 + ch, 8, count, deck);
        if generation == 3 {
            loops(&mut p, ch, deck);
        }
        if generation > 1 {
            meter(&mut p, ch, 2, deck);
        }
    }
    cc14(&mut p, 6, 0x1f, MidiTarget::Xfader);
    browse(&mut p, Some(6));
    if generation < 3 {
        // Official SB/SB2 DJ App Connect: any data2 value indicates connected.
        p.initialize.push(packet([0x9b, 9, 0]));
        p.notes.push_str(" Master/headphone volume knobs are handled by the hardware; HOT CUE 5–8 bank is unmapped.");
    } else {
        software_volume(&mut p);
    }
    p
}
fn rb() -> ControllerPreset {
    let mut p = pioneer(
        "DDJ-RB",
        "DDJ-RB_MIDI_Message_List_E.pdf",
        "E (no revision printed)",
        "pp. 1–3: deck, browser, mixer, pads and meters",
        "Two decks. Select HOT CUE mode; Shift + pad clears a cue. Shift + LOOP OUT toggles the current mixless loop. FX and other pad modes are unmapped. Browser-push channel is ambiguous in the official table and is left unmapped. Jog calibration and hardware testing pending.",
    );
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        transport(&mut p, ch, deck, &[0x21, 0x22, 0x23, 0x1f, 0x26]);
        mixer(&mut p, ch, deck, true);
        pads(&mut p, 7 + ch, 7 + ch, 8, 8, deck);
        note(&mut p, ch, 0x4d, MidiTarget::Loop { deck });
        meter(&mut p, ch, 2, deck);
    }
    cc14(&mut p, 6, 0x1f, MidiTarget::Xfader);
    software_volume(&mut p);
    // Manufacturer says channel 1/2 for the single browser push; don't guess 7.
    browse(&mut p, None);
    p
}
fn rev1() -> ControllerPreset {
    let mut p = pioneer(
        "DDJ-REV1",
        "DDJ-REV1_MIDI_Message_List_E1.pdf",
        "E1 (no revision printed)",
        "pp. 1–5: browse, deck, mixer, filter and hot cue pads",
        "Use deck selection 1/2 and HOT CUE mode. Shift + pad clears a cue. Decks 3/4, Tracking Scratch, Scratch Bank and effect paddles are unmapped. Jog calibration and hardware testing pending.",
    );
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        transport(&mut p, ch, deck, &[0x21, 0x22, 0x23, 0x1f, 0x26]);
        mixer(&mut p, ch, deck, true);
        pads(&mut p, 7 + ch * 2, 8 + ch * 2, 0, 8, deck);
        loops(&mut p, ch, deck);
        note(&mut p, ch, 0x17, MidiTarget::Vinyl { deck });
    }
    cc14(&mut p, 6, 0x1f, MidiTarget::Xfader);
    software_volume(&mut p);
    browse(&mut p, Some(6));
    p
}
fn wego3() -> ControllerPreset {
    let mut p = pioneer(
        "DDJ-WeGO3",
        "DDJ-WeGO3_List_of_MIDI_Message_E.pdf",
        "E (no revision printed)",
        "pp. 1–2: browser, deck, mixer and LEDs",
        "Two decks, four hot cues per deck; Shift + cue clears it. Loop controls adjust mixless's current loop. Effects and sampler mode are unmapped. Hardware controls master/headphone volume. Jog calibration and hardware testing pending.",
    );
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        transport(&mut p, ch, deck, &[0x21, 0x22, 0x26, 0x27]);
        loops(&mut p, ch, deck);
        for index in 0..4 {
            lit_note(&mut p, ch, 0x2e + index, MidiTarget::Cue { deck, index });
            note(
                &mut p,
                ch,
                0x5f + index,
                MidiTarget::ClearCue { deck, index },
            );
        }
        // WeGO3 mixer is on channel 7; do not reuse DDJ deck-channel EQ/faders.
        for (number, band) in [(7, EqBand::High), (11, EqBand::Mid), (15, EqBand::Low)] {
            cc14(&mut p, 6, number + ch, MidiTarget::Eq { deck, band });
        }
        cc14(&mut p, 6, 0x13 + ch * 2, MidiTarget::Fader { deck });
        lit_note(&mut p, 6, 0x54 + ch, MidiTarget::Pfl { deck });
    }
    cc14(&mut p, 6, 0x1f, MidiTarget::Xfader);
    browse(&mut p, Some(6));
    p
}

fn protect_shift(p: &mut ControllerPreset) {
    let shared = matches!(p.name.as_str(), "DDJ-REV1" | "DDJ-WeGO3");
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        let layer = if shared {
            "shift".into()
        } else {
            format!("shift-{ch}")
        };
        guard_deck_notes(p, deck, &layer, Some(0x67));
        if !shared {
            modifier(p, ch, 0x3f, &layer);
        }
    }
    if shared {
        modifier(
            p,
            6,
            if p.name == "DDJ-WeGO3" { 0x4f } else { 0x3f },
            "shift",
        );
    }
    for b in &mut p.bindings {
        if (p.name == "DDJ-RB" && matches!(b.target, MidiTarget::Loop { .. }))
            || (p.name == "DDJ-REV1" && matches!(b.target, MidiTarget::Vinyl { .. }))
        {
            if let Some(layer) = &mut b.layer {
                layer.active = true;
            }
        }
    }
}
