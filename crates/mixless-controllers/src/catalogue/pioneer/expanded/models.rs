//! Each row is a separately reviewed model, not a manufacturer-name fallback.
use super::Model;
const PADS: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
const CLASSIC: &[u8] = &[0x21, 0x22, 0x23, 0x1f, 0x26];
const MODERN: &[u8] = &[0x21, 0x22, 0x23, 0x29, 0x26];
const VOLUMES: &[u8] = &[8, 12, 13];

// Standard transport/mixer controls below were read in each cited table.
// Per-model exceptions are explicit columns and overrides, including absence.
macro_rules! model {
    ($name:literal, $file:literal, $pages:literal, $jog:expr, $modern:expr, $loop:expr, $sizes:expr, $vol:expr, $push:expr, $notes:literal $(, $field:ident: $value:expr)*) => {
        Model {
            $($field: $value,)*
            ..Model {
                name: $name, file: $file, pages: $pages, jog: $jog,
                touch: true, shared_shift: None, sync: 0x58, cue: true,
                pfl: Some(0x54), mixer: true, modern_pads: $modern,
                pad_notes: PADS, pad_on: 127, loop_note: $loop, loop_sizes: $sizes,
                key_lock: None, volumes: $vol, browse_push: $push, connect: None, disconnect: None, notes: $notes,
            }
        }
    };
}

pub(super) const MODELS: &[Model] = &[
    model!("DDJ-1000", "DDJ-1000_MIDI_Message_List_E1.pdf", "pp. 1–2, 4–7: browser, deck, mixer, hot cue pads", &[0x21,0x22,0x23,0x29,0x1f,0x26], true, Some(0x14), None, VOLUMES, false,
        "Use the first HOT CUE page. Loop IN/OUT and display commands are unmapped.", key_lock: Some(0x1a), pad_on: 1),
    model!("DDJ-1000SRT", "DDJ-1000SRT_MIDI_Message_List_E1.pdf", "pp. 1–2, 4–6: browser, deck, mixer, hot cue pads", CLASSIC, false, Some(0x14), None, VOLUMES, false,
        "Browser encoder press loads the selected track into its deck. Grid-edit messages are unmapped.", key_lock: Some(0x1a), pad_on: 1),
    model!("DDJ-800", "DDJ-800_MIDI_Message_List_E1.pdf", "pp. 1–2, 4–7: browser, deck, mixer, hot cue pads", &[0x21,0x22,0x23,0x29,0x1f,0x26], true, Some(0x4d), None, VOLUMES, false,
        "Use the first HOT CUE page. RELOOP/EXIT toggles the current mixless loop.", key_lock: Some(0x1a), pad_on: 1),
    model!("DDJ-FLX6", "DDJ-FLX6_MIDI_message_List_E1.pdf", "pp. 1–2, 4–7: deck, mixer, browser, hot cue pads", MODERN, true, Some(0x4d), None, VOLUMES, true,
        "Select the utility mode for other DJ software. Merge FX, Jog Cutter and Sample Scratch are unmapped.", pad_on: 127),
    model!("DDJ-FLX6-GT", "DDJ-FLX6_MIDI_message_List_E1.pdf", "Shared FLX6 list, pp. 1–2, 4–7: deck, mixer, browser, hot cue pads", MODERN, true, Some(0x4d), None, VOLUMES, true,
        "Select the utility mode for other DJ software. Merge FX, Jog Cutter and Sample Scratch are unmapped.", pad_on: 127),
    model!("DDJ-FLX10", "DDJ-FLX10_MIDI_Message_List_E1.pdf", "pp. 1–2, 4–5: browser, deck, mixer, hot cue pads", &[0x21,0x22,0x23,0x29,0x1f,0x26], true, None, None, VOLUMES, false,
        "Use the first HOT CUE page. Part separation, Part FX, jog displays and manual loop controls are unmapped.", key_lock: Some(0x1a), pad_on: 1),
    model!("DDJ-GRV6", "DDJ-GRV6_MIDI_Message_List_E1.pdf", "pp. 1–4: browser, deck, mixer, filter, hot cue pads", MODERN, true, Some(0x4d), None, VOLUMES, true,
        "Groove Circuit, drum replacement and other pad banks are unmapped.", key_lock: Some(0x1a)),
    model!("DDJ-REV5", "DDJ-REV5_MIDI_message_List_E1.pdf", "pp. 1–6: browser, deck, mixer, Shift, hot cue pads", CLASSIC, true, Some(4), Some((1,3)), VOLUMES, true,
        "Disable LOOP options in the setting utility. Jog Touch is unmapped because the official table's CC type and Note status conflict. STEMS, BPM Transition and AUX are unmapped.",
        touch: false, shared_shift: Some(0x3f), sync: 0x0a, pfl: Some(7), key_lock: Some(9), pad_on: 1),
    model!("DDJ-REV7", "DDJ-REV7_MIDI_message_List_E1.pdf", "pp. 1–6: browser, deck, mixer, Shift, hot cue pads", &[0x22,0x29], true, Some(0x14), Some((0x12,0x13)), VOLUMES, true,
        "Disable LOOP options in the setting utility. START/STOP controls playback. Motor control, displays, Instant Scratch and the headphone cue fader are unmapped. Use cue monitoring in the workspace.",
        touch: false, shared_shift: Some(0x3f), cue: false, pfl: None, key_lock: Some(0x1a), pad_on: 1),
    model!("DDJ-RR", "DDJ-RR_MIDI_Message_List_E.pdf", "pp. 1–4: browser, deck, mixer, hot cue pads", CLASSIC, false, Some(0x14), Some((0x12,0x13)), VOLUMES, true,
        "Other pad modes and manual loop controls are unmapped.", key_lock: Some(0x1a), pad_on: 1),
    model!("DDJ-RX", "DDJ-RX_List_of_MIDI_Message_E.pdf", "pp. 1–4: deck, mixer, browser, hot cue pads", CLASSIC, false, Some(0x14), Some((0x12,0x13)), &[], true,
        "Master and headphone knobs are hardware controls. Select the computer input for channels 1/2.", connect: Some(0)),
    model!("DDJ-RZ", "DDJ-RZ_MIDI_Message_List_E.pdf", "pp. 1–3, 5: browser, deck, mixer, hot cue pads", CLASSIC, false, Some(0x14), Some((0x12,0x13)), VOLUMES, true,
        "Select the computer input for channels 1/2.", connect: Some(0), pad_on: 1),
    model!(
        "DDJ-SR",
        "DDJ-SR_List_of_MIDI_Messages_E.pdf",
        "pp. 1–3: deck, mixer, browser, hot cue pads",
        CLASSIC,
        false,
        Some(0x14),
        Some((0x12, 0x13)),
        &[],
        true,
        "Master and headphone knobs are hardware controls. Pad Plus and other pad banks are unmapped."
    ),
    model!("DDJ-SR2", "DDJ-SR2_MIDI_Message_E1.pdf", "pp. 1–4: browser, deck, mixer, hot cue pads", CLASSIC, false, Some(0x14), Some((0x12,0x13)), VOLUMES, true,
        "Pitch Play and other pad banks are unmapped.", key_lock: Some(0x1a), pad_on: 1),
    model!("DDJ-SX", "DDJ-SX_List_of_MIDI_Messages_E.pdf", "pp. 1–4: deck, mixer, browser, hot cue pads", CLASSIC, false, Some(0x14), Some((0x12,0x13)), &[], true,
        "Master and headphone knobs are hardware controls. Select the computer input for channels 1/2.", connect: Some(0)),
    model!("DDJ-SX2", "DDJ-SX2_List_of_MIDI_Message_E.pdf", "pp. 1–4: deck, mixer, browser, hot cue pads", CLASSIC, false, Some(0x14), Some((0x12,0x13)), &[], true,
        "Master and headphone knobs are hardware controls. Select the computer input for channels 1/2.", connect: Some(0)),
    model!("DDJ-SX3", "DDJ-SX3_MIDI_Message_List_E1.pdf", "pp. 1–4: deck, mixer, browser, hot cue pads", CLASSIC, false, Some(0x14), Some((0x12,0x13)), &[], true,
        "Master and headphone knobs are hardware controls. Select the computer input for channels 1/2.", connect: Some(127), disconnect: Some(0)),
    model!("DDJ-SZ", "DDJ-SZ_MIDI_Message_List_E.pdf", "pp. 1–3: deck, mixer, browser, hot cue pads", CLASSIC, false, Some(0x14), Some((0x12,0x13)), &[12,13], true,
        "Master LEVEL is unmapped because the official channel and status columns disagree. Select the computer input for channels 1/2.", connect: Some(0)),
    model!("DDJ-SZ2", "DDJ-SZ2_MIDI_Message_v100_E.pdf", "pp. 1–4: browser, deck, mixer, hot cue pads", CLASSIC, false, Some(0x14), Some((0x12,0x13)), &[12,13], true,
        "The per-channel MASTER LEVEL messages are unmapped; use the workspace master control. Select the computer input for channels 1/2.", key_lock: Some(0x1a), connect: Some(0), pad_on: 1),
    model!("DDJ-SP1", "DDJ-SP1_List_of_MIDI_Messages_E.pdf", "pp. 1–4: Sync, browser, hot cue pads, Shift", &[], false, None, None, &[], true,
        "Pad accessory. Use deck selection 1/2 and HOT CUE mode. There are no playback, jog, mixer or audio-interface mappings.",
        touch: false, shared_shift: Some(0x40), cue: false, pfl: None, mixer: false, pad_on: 127, connect: Some(127)),
    model!("DDJ-XP1", "DDJ-XP1_List_of_MIDI_Message_E1.pdf", "pp. 1–3, 5: Sync, loop, browser, pads, Shift", &[], true, Some(0x14), None, &[], true,
        "Pad accessory. Use the first eight HOT CUE pads. Remaining pads, Slide FX and other banks are unmapped. Use a separate audio device.",
        touch: false, shared_shift: Some(0x40), cue: false, pfl: None, mixer: false, key_lock: Some(0x1a)),
    model!("DDJ-XP2", "DDJ-XP2_MIDI_Message_List_E1.pdf", "pp. 1–4: Sync, loop, browser, PAD MODE 1, Shift", &[], true, Some(0x14), None, &[], true,
        "Pad accessory. Select PAD MODE 1. Physical pads 1–8 map to mixless cues 1–8. Remaining pads, Slide FX and other banks are unmapped. Use a separate audio device.",
        touch: false, shared_shift: Some(0x40), cue: false, pfl: None, mixer: false, pad_notes: &[12,13,14,15,8,9,10,11]),
];
