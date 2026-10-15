//! DDJ-FLX2 addresses read independently from AlphaTheta's E1 message table.
use super::*;
#[cfg(test)]
mod tests;

pub(super) fn profile() -> ControllerPreset {
    let mut p = pioneer(
        "DDJ-FLX2",
        "DDJ-FLX2_MIDI_Message_List_E1.pdf",
        "E1 (copyright 2024; no printed revision)",
        "p. 1: deck/jog, mixer and 14-bit controls; p. 2: hot cue pads and vinyl output",
        "Connect by USB MIDI. Use HOT CUE mode (Shift + BEAT SYNC, then pad 1); Shift + pad clears a cue. CFX controls the mixless filter. Browse and load in the workspace. Smart Fader, master CUE, other pad modes, shifted transport and fader start are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending.",
    );
    p.provenance.push(Provenance {
        url: "https://downloads.support.alphatheta.com/manuals/dj-controllers/DDJ-FLX2/DDJ-FLX2_DRI1917A_manual.pdf".into(),
        revision: "DRI1917A".into(),
        section: "pp. 77, 82, 143: Mac audio setup, pad mode selection and USB MIDI for other DJ applications".into(),
    });
    for (ch, deck) in [(0, DeckId::A), (1, DeckId::B)] {
        // FLX2 has no TRIM knob, browse encoder, loop button or level meter.
        // Use only messages present in this model's table.
        transport(&mut p, ch, deck, &[0x21, 0x22, 0x23, 0x29]);
        mixer(&mut p, ch, deck, false);
        pads(&mut p, 7 + ch * 2, 8 + ch * 2, 0, 8, deck);
        // This is an output-only firmware setting, not a physical VINYL button.
        led(&mut p, ch, 0x17, MidiTarget::Vinyl { deck });
    }
    cc14(&mut p, 6, 0x1f, MidiTarget::Xfader);
    software_volume(&mut p);
    // No inferred Master 1/2 / Cue 3/4 proposal: the manual identifies MASTER
    // and PHONES, but does not establish the numeric CoreAudio channel order.
    p
}
