//! Independently authored mixless profiles. Protocol facts are recorded in SOURCES.md.
//! Jog calibration is an application default pending physical measurement, not certification.
use crate::*;
use mixless_protocol::{DeckId, EqBand};

fn input(p: &mut ControllerPreset, address: MidiAddress, target: MidiTarget, mode: ValueMode) {
    let mut b = Binding::from_legacy(&MidiBinding {
        address: None,
        inverted: false,
        sensitivity: 1.,
        ticks_per_revolution: None,
        pickup: false,
        device_id: None,
        kind: MidiSourceKind::Note,
        channel: 0,
        number: 0,
        target,
        mode: MidiControlMode::Auto,
    });
    b.id = format!("control-{}", p.bindings.len());
    b.source = InputSource::Midi { address };
    b.mode = if mode == ValueMode::Auto && matches!(b.target, MidiTarget::TemporaryCue { .. }) {
        ValueMode::Momentary
    } else {
        mode
    };
    if matches!(b.target, MidiTarget::Jog { .. }) {
        b.ticks_per_revolution = Some(720.);
    }
    p.bindings.push(b);
}
fn note(p: &mut ControllerPreset, ch: u8, number: u8, target: MidiTarget) {
    input(
        p,
        MidiAddress::Note {
            channel: ch,
            number,
        },
        target,
        ValueMode::Auto,
    );
}
fn cc14(p: &mut ControllerPreset, ch: u8, msb: u8, target: MidiTarget) {
    input(
        p,
        MidiAddress::Cc14 {
            channel: ch,
            msb,
            lsb: msb + 32,
        },
        target,
        ValueMode::Absolute,
    );
}
fn led(p: &mut ControllerPreset, ch: u8, number: u8, target: MidiTarget) {
    let packet = OutputPacket {
        endpoint: "main".into(),
        data: vec![0x90 | ch, number, 0],
        report: None,
    };
    let mut shutdown = packet.clone();
    if matches!(target, MidiTarget::Vinyl { .. }) {
        shutdown.data[2] = 127;
    } // restore documented power-on mode
    p.shutdown.push(shutdown);
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
mod hercules;
mod native_instruments;
mod numark;
mod pioneer;

pub(crate) fn catalogue() -> Vec<ControllerPreset> {
    let mut profiles = pioneer::rekordbox_entry();
    profiles.extend(pioneer::additional());
    profiles.extend(hercules::profiles());
    profiles.push(numark::dj2go());
    profiles.extend(native_instruments::profiles());
    profiles
}

fn profile(
    model: &str,
    manufacturer: &str,
    url: &str,
    revision: &str,
    section: &str,
    notes: &str,
) -> ControllerPreset {
    let mut p = ControllerPreset::blank(model);
    p.id = format!(
        "builtin-{}-v1",
        model.to_ascii_lowercase().replace(' ', "-")
    );
    p.manufacturer = manufacturer.into();
    p.notes = notes.into();
    p.endpoints[0].matcher = DeviceMatcher::Midi {
        names: vec![model.into()],
    };
    p.provenance.push(Provenance {
        url: url.into(),
        revision: revision.into(),
        section: section.into(),
    });
    p
}
fn cc(p: &mut ControllerPreset, ch: u8, number: u8, target: MidiTarget, mode: ValueMode) {
    input(
        p,
        MidiAddress::Cc {
            channel: ch,
            number,
        },
        target,
        mode,
    );
}
fn feedback_note(p: &mut ControllerPreset, ch: u8, number: u8, target: MidiTarget) {
    let packet = OutputPacket {
        endpoint: "main".into(),
        data: vec![0x90 | ch, number, 0],
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
fn lit_note(p: &mut ControllerPreset, ch: u8, number: u8, target: MidiTarget) {
    note(p, ch, number, target.clone());
    feedback_note(p, ch, number, target);
}
fn meter(p: &mut ControllerPreset, ch: u8, number: u8, deck: DeckId) {
    let packet = OutputPacket {
        endpoint: "main".into(),
        data: vec![0xb0 | ch, number, 0],
        report: None,
    };
    p.shutdown.push(packet.clone());
    p.feedback.push(FeedbackRule {
        signal: FeedbackSignal::Meter { deck: Some(deck) },
        packet,
        bit_offset: 16,
        bits: 7,
        little_endian: true,
        off: 0,
        on: 127,
    });
}
fn packet(data: [u8; 3]) -> OutputPacket {
    OutputPacket {
        endpoint: "main".into(),
        data: data.into(),
        report: None,
    }
}

// Some firmware changes the address of a held button when Shift changes.
// Layers release the old hold and reset its edge even if its note-off never arrives.
fn guard_deck_notes(
    p: &mut ControllerPreset,
    deck: DeckId,
    layer: &str,
    shifted_touch: Option<u8>,
) {
    for b in &mut p.bindings {
        if b.target.deck() != Some(deck) {
            continue;
        }
        if let InputSource::Midi {
            address: MidiAddress::Note { number, .. },
        } = b.source
        {
            let active = matches!(b.target, MidiTarget::ClearCue { .. })
                || (matches!(b.target, MidiTarget::JogTouch { .. })
                    && Some(number) == shifted_touch);
            b.layer = Some(LayerCondition {
                name: layer.into(),
                active,
            });
        }
    }
}
fn modifier(p: &mut ControllerPreset, ch: u8, number: u8, layer: &str) {
    note(p, ch, number, MidiTarget::Master);
    p.bindings.last_mut().unwrap().sets_layer = Some(layer.into());
}
