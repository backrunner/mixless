---
title: "Controllers"
description: "Map MIDI and HID hardware to the decks, mixer, and library."
order: 6
image: /images/social.png
imageAlt: "mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

Open **Preferences → Controllers**. Select a device and a named preset, then choose **Apply**. Discovery runs in the background every 500 ms, including while Preferences is closed. Existing assignments reconnect by device ID; an unknown or ambiguous device requires an explicit choice. An offline preset stays in the library until you apply it.

## Preset library

Use **Manage → New preset…**, **Duplicate…**, **Rename…**, and **Delete…** to manage user presets. Built-ins are read-only; editing creates a user copy. Editing a shared preset for one device creates a separate copy. Applying that copy is explicit. An assigned preset must be unassigned before deletion.

**Manage → Import…** previews the device requirements, number of bindings and naming conflicts. Confirmation saves a separate preset without changing device assignments. **Manage → Export…** writes only the selected native JSON preset, without local device IDs. Unsupported versions, invalid rules and failed saves leave the active configuration intact. Raw scripts and XML mappings are not imported. Community HID presets may include a sandboxed Wasm adapter with explicit report permissions.

Existing `midi.json` is backed up as `midi.pre-controllers.backup.json` and migrated once into `midi.controllers.json`. The original is retained. Explicit legacy wildcard bindings remain supported.

## Mapping and monitoring

The controller layout stays visible above the mapping list; select a control or row to inspect its input. For MIDI learning, choose the input device, click a deck/mixer control, then move its physical control. Offline diagram clicks only inspect mappings. The first compatible Note, CC or Pitch Bend is saved. Cancel stops learning without changing the mapping. The editor supports Note, CC, Pitch Bend and explicitly configured 14-bit CC MSB/LSB pairs. It does not infer pairs. CC supports relative 1/127, 65/63 or 1/65 modes; Calibration exposes direction, sensitivity, pickup and jog ticks per turn. Complex mappings show their actual input; use Edit MIDI / HID to change their rules. Preset notes and Input monitor are optional under Details.

**Manage → Edit MIDI / HID…** edits the selected preset's declarative JSON: pitch bend, paired 14-bit CC, RPN/NRPN, bounded SysEx, calibration, soft takeover, device-local layers and hold/toggle behavior. Jog resolution is configurable with `ticks_per_revolution`; its application default must be calibrated against real hardware. **Details → Ports…** assigns each logical port to an explicit local input/output ID.

HID presets specify VID/PID, interface and Usage, report ID, field offset/width, signedness, byte order and counter mode. Under **Details**, **Add HID field** creates an editable field; **Read feature…** reads the requested report ID and length. **Input monitor** displays received bytes. Field offsets include a report-ID byte; unnumbered reports use a leading zero. The meanings of fields must come from manufacturer information or your own measurements.

Feedback rules write MIDI or HID output/feature reports from current application state. Repeated updates are deduplicated and sent at at most 30 Hz per rule. Reconnecting initializes the device and refreshes feedback. Disconnecting releases held controls. Audio performance controls run outside the display loop; library browsing and loading wake the application event loop in order.

## Community presets

Import a community preset through **Manage → Import…**. The preview shows its author, version, license and device requirements. An adapter also lists its allowed output and feature reports. Import saves a separate copy; applying it to a device is a separate action. New versions do not replace an active preset automatically. Keep the previous copy to switch back.

Fixed report layouts use declarative mappings. More complex HID protocols can use the independent Wasm adapter API for checksums, handshakes, sequence tracking and timers. Adapter mappings appear as **Adapter** in the control list; their source is maintained by the author. The adapter has no file or network access. If it fails, the host stops it and releases held controls; apply the preset again to restart it.

Authors can use the [controller SDK and examples](https://github.com/backrunner/mixless/tree/main/crates/mixless-controllers) to validate, package, record and replay reports. Fixtures check expected actions and feedback bytes without hardware. Imported publisher test claims do not grant hardware-verified status. Source and license information must accompany community packages.

## Included profiles and evidence

The preset library includes **14 independently authored profiles**: twelve based on manufacturer MIDI tables and two requiring named NI Controller Editor templates:

| Manufacturer | Models |
| --- | --- |
| Pioneer DJ / AlphaTheta | DDJ-200, DDJ-400, DDJ-FLX4, DDJ-SB, DDJ-SB2, DDJ-SB3, DDJ-RB, DDJ-REV1, DDJ-WeGO3 |
| Hercules | DJControl Inpulse 200, DJControl Inpulse 300 (original generations) |
| Numark | DJ2GO (original model) |
| Native Instruments | F1 (FL Studio template), X1 MK1 (SSL 2.2 template) |

Profiles cover documented transport, mixing, jog, browsing, hot cues and feedback as available on each model. Open **Details → Preset notes** for setup instructions and coverage limits. Use decks 1/2 on SB2/SB3/REV1; SB/SB2, WeGO3 and Inpulse 200 expose four hot cues per deck. Other pad modes and effects are not implied. Newer model generations need their own profiles.

All included profiles are labelled **protocol implemented**, with **hardware testing pending**. Jog sensitivity is provisional. Physical verification can follow later; inclusion does not mean the device is certified. Separate **simulation tested** and **hardware tested** evidence labels are available. Configure audio channels in Audio settings; selecting a profile does not switch outputs.

## Controller audio

In **Audio I/O**, select the Master and Headphones devices and channel pairs. A four-channel controller can use Master 1/2 and Cue 3/4 in one stream. Overlapping or unavailable channels are rejected, and unused channels remain silent. **Details → Audio routing…** fills in a profile's recommendation for review; **Apply** in Audio I/O is required to change output. A headphone-device failure never redirects Cue into Master.

**Headphone mix** blends Cue into Master. At 0% Master it preserves Cue-only monitoring; at 100% Master it monitors the main output. Intermediate positions blend linearly. Headphone volume applies after blending and does not change the main output. Map it through **CUE MIX** in the diagram.

## NI DJ devices

F1 and original X1 presets require the named unmodified Controller Editor 2.8.2 template, Basic page and MIDI mode. These are not factory addresses or mappings for later generations or shifted pages. F1 offers two sets of deck switches, stem gains and pad brightness feedback; X1 offers transport, FX/EQ, browsing and loading without light feedback. Preset notes explain the layout.

MX2 supports Learn after switching to MIDI mode with both Shift buttons. There is no complete MX2 preset or NI native-protocol adapter yet. X1 MK3, Z1 MK2 and other NI models still have message-definition gaps and are not advertised as hardware-compatible.

Traktor's per-device output configuration provides audio recommendations for MX2, Z1 (original and MK2), Z2, S2 and S4 (original, MK2 and MK3), S3, S5 and S8. With a matching device selected, **Details → Audio routing…** fills in Master 1/2 and Cue 3/4 without requiring a control preset. Review and explicitly apply it in Audio I/O. This does not add control mappings or verify physical outputs. See the [NI compatibility matrix](https://github.com/backrunner/mixless/blob/main/crates/mixless-controllers/NI_COMPATIBILITY.md).
