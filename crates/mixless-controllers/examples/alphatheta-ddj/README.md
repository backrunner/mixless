# DDJ controller expansion

Reviewed on 2026-10-13. This batch adds 24 DJ-controller profiles; standalone
mixers and decks are excluded. The catalogue now has 39 profiles, including 34
Pioneer DJ / AlphaTheta controllers. All remain `protocol`, with hardware checks
and jog calibration pending. No manufacturer PDFs or other DJ applications'
mapping logic are redistributed.

The [review record](protocol-review.json) retains each official download URL,
PDF SHA-256, selected sections and omissions. [Replays](replays.json) contain
24 independently authored synthetic sessions with 2,420 asserted frames. They
test both decks, exact pad/Shift channels, normal and shifted cues, repeated
button presses, note-off, paired faders, jog direction, unsupported banks and
shutdown output. They open no hardware.

| Controller | Official MIDI list | Scope and limits |
| --- | --- | --- |
| DDJ-1000 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-1000/DDJ-1000_MIDI_Message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Use the first HOT CUE page. Loop IN/OUT and display commands are unmapped. |
| DDJ-1000SRT | [PDF](https://downloads.support.alphatheta.com/firmwares/dj-controllers/DDJ-1000SRT/DDJ-1000SRT_MIDI_Message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Browser encoder press loads the selected track into its deck. Grid-edit messages are unmapped. |
| DDJ-800 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-800/DDJ-800_MIDI_Message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Use the first HOT CUE page. RELOOP/EXIT toggles the current mixless loop. |
| DDJ-FLX10 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-FLX10/DDJ-FLX10_MIDI_Message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Use the first HOT CUE page. Part separation, Part FX, jog displays and manual loop controls are unmapped. |
| DDJ-FLX6-GT | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-FLX6/DDJ-FLX6_MIDI_message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Select the utility mode for other DJ software. Merge FX, Jog Cutter and Sample Scratch are unmapped. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-FLX6 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-FLX6/DDJ-FLX6_MIDI_message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Select the utility mode for other DJ software. Merge FX, Jog Cutter and Sample Scratch are unmapped. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-GRV6 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-GRV6/DDJ-GRV6_MIDI_Message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Groove Circuit, drum replacement and other pad banks are unmapped. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-REV5 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-REV5/DDJ-REV5_MIDI_message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Disable LOOP options in the setting utility. Jog Touch is unmapped because the official table's CC type and Note status conflict. STEMS, BPM Transition and AUX are unmapped. |
| DDJ-REV7 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-REV7/DDJ-REV7_MIDI_message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Disable LOOP options in the setting utility. START/STOP controls playback. Motor control, displays, Instant Scratch and the headphone cue fader are unmapped. Use cue monitoring in the workspace. |
| DDJ-RR | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-RR/DDJ-RR_MIDI_Message_List_E.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Other pad modes and manual loop controls are unmapped. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-RX | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-RX/DDJ-RX_List_of_MIDI_Message_E.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Master and headphone knobs are hardware controls. Select the computer input for channels 1/2. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-RZ | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-RZ/DDJ-RZ_MIDI_Message_List_E.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Select the computer input for channels 1/2. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-SP1 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SP1/DDJ-SP1_List_of_MIDI_Messages_E.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Pad accessory. Use deck selection 1/2 and HOT CUE mode. There are no playback, jog, mixer or audio-interface mappings. |
| DDJ-SR | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SR/DDJ-SR_List_of_MIDI_Messages_E.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Master and headphone knobs are hardware controls. Pad Plus and other pad banks are unmapped. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-SR2 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SR2/DDJ-SR2_MIDI_Message_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Pitch Play and other pad banks are unmapped. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-SX | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SX/DDJ-SX_List_of_MIDI_Messages_E.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Master and headphone knobs are hardware controls. Select the computer input for channels 1/2. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-SX2 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SX2/DDJ-SX2_List_of_MIDI_Message_E.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Master and headphone knobs are hardware controls. Select the computer input for channels 1/2. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-SX3 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SX3/DDJ-SX3_MIDI_Message_List_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Master and headphone knobs are hardware controls. Select the computer input for channels 1/2. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-SZ | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SZ/DDJ-SZ_MIDI_Message_List_E.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Master LEVEL is unmapped because the official channel and status columns disagree. Select the computer input for channels 1/2. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-SZ2 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SZ2/DDJ-SZ2_MIDI_Message_v100_E.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. The per-channel MASTER LEVEL messages are unmapped; use the workspace master control. Select the computer input for channels 1/2. Browser push is unmapped; use the deck LOAD buttons. |
| DDJ-WeGO2 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-WeGO2/DDJWeGO2ListofMIDIMessagealgoriddimE.pdf) | Two decks, four hot cues per deck. Select HOT CUE mode; Shift + cue clears it. Hardware controls master/headphone volume. Other pad modes, effects and fader start are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. The loop-size encoder is unmapped. |
| DDJ-WeGO4 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-WeGO4/DDJ-WeGO4_List_of_MIDI_Message_E.pdf) | Two decks, four hot cues per deck. Select HOT CUE mode; Shift + cue clears it. Hardware controls master/headphone volume. Other pad modes, effects and fader start are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. |
| DDJ-XP1 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-XP1/DDJ-XP1_List_of_MIDI_Message_E1.pdf) | Decks 1/2 only. Select HOT CUE mode; Shift + pad clears a cue. Other pad banks, effects, fader start, displays and hardware audio routing are unmapped. Choose audio outputs manually. Jog calibration and hardware testing pending. Pad accessory. Use the first eight HOT CUE pads. Remaining pads, Slide FX and other banks are unmapped. Use a separate audio device. |
| DDJ-XP2 | [PDF](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-XP2/DDJ-XP2_MIDI_Message_List_E1.pdf) | Decks 1/2 only. Select PAD MODE 1; Shift + pad clears a cue. Pad accessory. Select PAD MODE 1. Physical pads 1–8 map to mixless cues 1–8. Remaining pads, Slide FX and other banks are unmapped. Use a separate audio device. Hardware testing pending. |

FLX6-GT uses the FLX6 table only because the [official compatibility article](https://support.alphatheta.com/en-us/articles/4404631697945) explicitly covers both models. The firmware utility must be set for other DJ software.

Pad output values are model-specific: binary LEDs receive 127; colour LEDs use
a declared palette entry. REV5 Sync/PFL/loop addresses differ from other DDJs.
Its Jog Touch rows label the message CC while encoding a Note status; touch
is omitted. REV7 has no assumed conventional Cue/touch packet or motor/display
control. XP2 physical pads 1–8 send notes 12–15 then 8–11 in PAD MODE 1.
WeGO2 mixer messages use the global channel; WeGO4 has no middle EQ band.

Browser encoder rotation and explicit deck LOAD are mapped. A global Shift
packet guards encoder push where supported; push remains unmapped on controllers
with separate deck Shift packets. No audio channel order is inferred from MIDI.

SX3 uses the documented General DJ Connect ON packet on connection and OFF
on disconnection. Other models' connection packets are not treated as binary
switches unless their own table declares that behavior.

## Deferred controllers

- Original WeGO: its retrieved official table does not establish a Shift packet.
  Reusing WeGO2's packet could leave held controls stuck; no profile is added.
- DDJ-ERGO, DDJ-T1 and DDJ-S1: official numeric lists or embedded manual tables
  have not been retrieved and reviewed in this batch. No addresses are guessed.
- Other manufacturers and NI generations need independently reviewed numeric
  facts or device captures; see [NI research](../../NI_COMPATIBILITY.md).

## Replay

Run `cargo test --locked -p mixless-controllers reviewed_model_replays`.
To inspect a single session, export the built-in profile, extract its trace from
`replays.json` and use `mixless-controller replay PRESET TRACE --check`.
