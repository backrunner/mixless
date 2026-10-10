# Controller implementation provenance

All controller service, schema, decoder, routing, UI and fixtures are mixless
implementations. No code, scripts, mappings, assets or tests from Mixxx may be
added or converted into this catalogue. Historical derived presets and the XML
import path have been removed. Native JSON import does not execute scripts.

## Manufacturer protocol sources

Retrieved 2026-10-09 from the manufacturers' official download services. Documents
are not bundled. Numeric protocol addresses are independently assigned to mixless
commands in `src/catalogue/`; the implementation is not a translated mapping.
The catalogue contains 12 profiles across three manufacturers, all labelled
`protocol`; hardware validation is deferred, not implied by inclusion.

| Profile | Source | Revision / sections | Evidence |
| --- | --- | --- | --- |
| DDJ-400 | [Official MIDI message list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-400/DDJ-400_MIDI_Message_List_E1.pdf) | 1.00; p. 1 deck/browser and jog encoding; p. 2 mixer, meters and pads 1–4; p. 3 pads 5–8 and vinyl output | Protocol implemented; synthetic routing validation; no physical unit |
| DDJ-FLX4 | [Official MIDI message list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-FLX4/DDJ-FLX4_MIDI_message_List_E1.pdf) | 1.0; p. 1 deck/jog; p. 2 mixer/browser; pp. 3–5 hot cue pads; p. 5 vinyl output | Protocol implemented; synthetic routing validation; no physical unit |

The additional independently authored profiles are:

| Profile | Official source | Revision / sections | Implemented scope and limits |
| --- | --- | --- | --- |
| DDJ-200 | [MIDI list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-200/DDJ-200_MIDI_Message_List_E2.pdf) | E2, 2019; p. 2 | Transport, touch/jog, tempo, EQ/filter/faders, PFL, eight cues, LEDs and vinyl mode. No audio interface, browse controls or Transition FX. |
| DDJ-SB | [MIDI list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SB/DDJ_SB_List_of_MIDI_Messages_E.pdf) | 1.00, 2013; pp. 1–2 | Transport, touch/jog, tempo, EQ/filter/faders, PFL, key lock/vinyl, browse/load, four hot cues, LEDs and DJ App Connect. Master/headphone level is hardware controlled. Other pad banks and Filter Fade omitted. |
| DDJ-SB2 | [MIDI list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SB2/DDJ-SB2_List_of_MIDI_Messages_E.pdf) | 1.00, 2015; pp. 1–3 | SB scope plus trim and meters; decks 1/2 only. Four hot cues in the normal bank; HOT CUE 5–8 and other pad banks omitted. |
| DDJ-SB3 | [MIDI list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-SB3/ddj-sb3_midi_message_list_e2.pdf) | 1.01, 2020; pp. 1–3 | Transport, touch/jog, tempo, mixer, PFL, key lock/vinyl, loops, browse/load, eight cues, LEDs/meters. Decks 1/2 only. Pad Scratch, FX Fade and other pad banks omitted. |
| DDJ-RB | [MIDI list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-RB/DDJ-RB_MIDI_Message_List_E.pdf) | E, no printed revision; pp. 1–3 | Transport, touch/jog, tempo, mixer/filter, browse/load, eight cues, LEDs/meters. Shift + LOOP OUT toggles the current loop. The browser push has conflicting channel information; it is omitted. |
| DDJ-REV1 | [MIDI list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-REV1/DDJ-REV1_MIDI_Message_List_E1.pdf) | E1, no printed revision; pp. 1–5 | Transport, touch/jog, tempo, mixer/filter, loops, browse/load, eight cues and documented LEDs; use decks 1/2. Decks 3/4, Tracking Scratch, Scratch Bank and FX paddles omitted. |
| DDJ-WeGO3 | [MIDI list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-WeGO3/DDJ-WeGO3_List_of_MIDI_Message_E.pdf) | E, no printed revision; pp. 1–2 | Transport, touch/jog, tempo, EQ/faders, PFL, loops, browse/load, four cues and LEDs. Mixer uses channel 7; no inferred compatibility with other WeGO generations. Effects and sampler mode omitted. |
| DJControl Inpulse 200 | [MIDI commands](https://ts.hercules.com/download/sound/manuals/DJC_Inpulse200/DJControlInpulse200_MIDI_Commands_V_1_0.pdf) | 1.0; PDF pp. 4–19 inputs, 20–23 LEDs, 29 status request | Transport, touch/jog, tempo, two-band EQ/filter/faders, master/headphone volume, PFL, browse/load, four cues and LEDs. Original generation only; no MK2/MK3 claim. |
| DJControl Inpulse 300 | [MIDI commands](https://ts.hercules.com/download/sound/manuals/DJC_Inpulse300/DJControlInpulse300_MIDI_Commands_V_1_2.pdf) | 1.2; PDF pp. 4–39 inputs, 40–54 LEDs | Transport, touch/jog, tempo, trim/three-band EQ/filter/faders, master/headphone volume, PFL, browse/load, eight cues and LEDs. Original generation only; no MK2 claim. |
| Numark DJ2GO | [UI and Communications Specification](https://www.numark.com/images/product_downloads/dj2go___software_definition_and_midi_spec_v1.0.pdf) | Rev B; pp. 5–7 | Transport, tempo, jog, volume/crossfader, master/headphones, browse/load and eight LEDs. No touch sensor, pads or built-in audio route. Pitch-bend buttons and BACK are omitted. DJ2GO2/Touch are different protocols. |

### Protocol distinctions and application decisions

- Pioneer and Hercules profiles use paired 14-bit controls; DJ2GO pots/faders use
  7-bit CC. Pioneer jog uses binary offset; Hercules and DJ2GO use two's complement.
  **720 ticks/revolution is a mixless starting calibration**, not a manufacturer
  resolution claim. It must be measured before hardware verification.
- SB/SB2/SB3/RB pad channels are 7/8 (zero-based), while 200/400/FLX4/REV1 use
  7/9 with separate shift channels. Hercules pads use 6/7. The differing pad banks,
  mixer channels and shift addresses are explicit in the declarations.
- Deck CUE is momentary temporary cue. Normal hot-cue pads trigger/store cues;
  shifted pads clear them. Shift changes release held controls and reset button
  edges on that device, even if firmware sends the release on another address.
- Browse selects tracks; shifted browse selects playlists; browse press loads the
  focused deck where its address is unambiguous. Loop Exit/OUT toggles mixless's
  current loop length; loop half/double changes that length. No other software's
  product semantics are imported.
- Hercules 200/300 tables contain inconsistent shifted deck-B note addresses and
  labels. Those shifted deck-button functions are omitted; normal deck controls,
  shifted CC controls and unambiguous shifted hot cues remain mapped. Activating
  Shift releases normal-layer Cue/Jog touch; shifted touch is not guessed.
- SB/SB2 use the documented DJ App Connect packet. Only Inpulse 200 sends its
  documented status-request packet on connection. Other initialization messages
  are not extrapolated across models. Feedback is sourced from mixless state;
  meter bytes are normalized levels, not calibrated hardware dB claims.
- No effects, sampler banks, motor control, screens or four-deck behavior is
  implied. Profiles have visible setup/coverage notes in the Controllers settings;
  those notes travel with native JSON exports.
- Exact model names are recommendations only. Users explicitly choose the device
  and profile; differing OS port names can be assigned manually. No substring
  match silently applies a profile to a newer generation or unrelated device.
- DDJ-400/FLX4 retain their existing user-reviewed Master 1/2 / Cue 3/4 proposal.
  New profiles do not invent audio channel layouts from their MIDI tables; use
  Audio settings to select the device/channel pairs. No profile auto-switches audio.

### Deferred evidence

All 12 profiles have software schema/routing checks, not physical-device evidence.
Physical control direction, jog calibration, pad-mode behavior, endpoint naming,
LED appearance and audio routing remain to be checked on real units.

The [Inpulse 500 official MIDI table](https://ts.hercules.com/download/sound/manuals/DJC_Inpulse500/DJControlInpulse500_MIDI_Commands.pdf)
was also reviewed (2022, pp. 2, 5 and 11). It assigns CC 08 both to tempo and jog
touch, and repeats the left fader MSB as its LSB. No Inpulse 500 profile is shipped
until independent capture or corrected manufacturer documentation resolves these
conflicts. This is a protocol-evidence gap, not a requirement to physically verify
otherwise unambiguous profiles before adding them.

## Generic protocols and tests

- [MIDI Association MIDI 1.0](https://midi.org/midi-1-0-core-specifications):
  channel messages, Note release, CC, pitch bend, RPN/NRPN and SysEx framing.
- [USB HID 1.11](https://www.usb.org/document-library/device-class-definition-hid-111):
  reports, report IDs and report descriptor framing. Transport uses hidapi;
  application fields are explicit user-authored declarations.
- Generic tests use synthetic values (for example report ID 3 with an 8-bit
  wrapping counter) invented for mixless, not captured from another project.
- CoreMIDI virtual-port tests validate real OS MIDI transport, feedback, hotplug,
  disconnection releases and dispatch latency with no UI polling.

No HID model is advertised as supported without manufacturer protocol evidence
or independently collected report traces. A report descriptor alone does not
establish the meaning of a button, platter, display or motor.

## Verification labels

`protocol` records implemented declarations; `simulation` requires retained
synthetic/virtual transport evidence; `hardware` requires a named device,
firmware, OS version, input/output/audio checks and measured jog calibration.
Imported and modified profiles are downgraded to `protocol`; user-provided
metadata is not an endorsement by mixless. Built-ins remain `protocol` until
physical verification. A test suite passing is not physical-device evidence.
