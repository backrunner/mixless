# DDJ-FLX2 USB MIDI preset

Independently authored mixless preset and synthetic expectations, MPL-2.0.
No third-party DJ application mapping, code or fixture is used.

Choose **DDJ-FLX2** in **Preferences → Controllers** and explicitly apply it
to the device's MIDI input. Connect by USB. Select HOT CUE mode with
Shift + BEAT SYNC, then pad 1. Shift + pad clears the corresponding cue.
Browse and load tracks in the workspace.

The preset covers both decks' Play, temporary Cue, Sync, platter touch/rotation,
tempo, three-band EQ, channel faders, CFX as the mixless filter, headphone PFL,
eight hot cues per deck, crossfader and master/headphone level. The documented
Play/Cue/Sync/PFL and hot cue LEDs follow software state. Vinyl is an
output-only firmware setting; shutdown restores its documented default of on.

Smart Fader, master headphone CUE, other pad banks, shifted transport, fader
start and load illumination are omitted. It does not inherit FLX4's TRIM,
browse controls, loops or meters. The manual names MASTER and PHONES outputs
without confirming their numeric CoreAudio order, so configure Audio I/O
manually. Bluetooth pairing, jog calibration, physical LEDs and audio routing
have not been tested on a DDJ-FLX2.

Sources reviewed on 2026-10-13:

- [Official E1 MIDI message list](https://downloads.support.alphatheta.com/software_info/dj-controllers/DDJ-FLX2/DDJ-FLX2_MIDI_Message_List_E1.pdf),
  pp. 1–2. SHA-256: `cccd405d9470382b58ab5d4e421678288448e1209e127d40c60f5228590b0e21`.
- [Official DRI1917A manual](https://downloads.support.alphatheta.com/manuals/dj-controllers/DDJ-FLX2/DDJ-FLX2_DRI1917A_manual.pdf),
  pp. 77, 82 and 143: audio setup, pad modes and other DJ applications via USB MIDI.

`fixture.json` contains independently specified input messages, expected actions,
unmapped messages, feedback deduplication and shutdown expectations. These are
synthetic protocol checks, not device captures. `trace.json` is pinned only after
all expectations pass. The preset remains `protocol`.

```sh
target/debug/mixless-controller validate crates/mixless-controllers/examples/ddj-flx2/preset.json
target/debug/mixless-controller replay crates/mixless-controllers/examples/ddj-flx2/preset.json crates/mixless-controllers/examples/ddj-flx2/trace.json --check
```
