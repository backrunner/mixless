---
title: "MIDI controllers"
description: "Map your hardware to the decks, mixer, and library."
order: 6
image: /images/social.png
imageAlt: "mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

Open **Preferences → MIDI Mapping**. The page lists all supported controls, including unmapped ones. Mapped controls are highlighted in the deck/mixer wireframe.

## Assign a control

1. Enable the MIDI input device. Use **Apply inputs** to reconnect after changing devices.
2. Click a control in the wireframe. The message above it shows which control is waiting for input.
3. Turn a hardware knob or fader, or press a button. The first compatible signal is saved automatically; the control turns green. There is no separate **Assign** step.

Click another control to bind it next. **Cancel** or Escape stops waiting without changing the mapping. Button releases and jog touch notes are ignored when learning a rotation. If the captured device/channel/CC or note already controls another target, its binding moves to the selected target and the message identifies the previous one.

For manual setup, select a control in the list, choose its device, channel, and CC or note, then press **Assign**. **Learn** in the editor captures a signal for review before **Assign**. Performance commands are suppressed while learning.

## Encoders and knobs

Jog wheels and library/browser encoders support relative **1/127**, **65/63**, and **1/65** modes. Other knobs also support absolute CC. **Auto** preserves the legacy absolute-knob and two's-complement-jog behavior.

Choose the mode that matches your controller's output. If an encoder jumps unexpectedly or moves in the wrong direction, check the device's MIDI documentation and mapping mode.

## Library controls

Browser encoders can select tracks or playlists. Map separate load buttons for deck A, deck B, or the focused deck. This lets you browse and load without reaching for the mouse.

## Save and reuse

Clear individual assignments, or import/export mappings as JSON. Preferences and mappings persist beside the library database in `preferences.json` and `midi.json`.

Legacy maps without a device ID match any enabled input. Set an explicit device when using multiple controllers to keep their assignments distinct.
