# Native Instruments DJ controllers

Offline review, 2026-10-12. No NI device was opened, configured, captured or
physically tested. A MIDI-mode switch is evidence of an available interface,
not evidence of a complete message map. Generations are separate protocols.

The same implementation agent inspected vendor definitions before writing the
NI integration. Research and implementation were not separated; this work must
not be labelled verified strict clean room.

## Traktor evidence and coverage boundaries

Traktor's official manuals and installed device definitions are the primary
research sources for NI integration. Controller Editor templates provide
supplementary, template-specific MIDI addresses. The included F1 and X1 MK1
profiles are **not Traktor factory mappings** and do not implement those devices'
Traktor native modes. Neither profile establishes native compatibility.

The [Traktor controller setup guide](https://support.native-instruments.com/support/solutions/articles/69000879434-traktor-controller-setup-guide)
separates NI devices, third-party Plug & Play devices, Traktor Compatible devices
and generic USB MIDI controllers. Those categories require different evidence:

| Traktor material | What it establishes | What remains to implement or verify |
| --- | --- | --- |
| Manuals and mapping guides | Control behavior, modes and documented setup | Numeric messages if omitted, mixless feature equivalents and hardware checks |
| Factory `.tsi` files | Traktor configuration data for particular devices/layouts | Decode the container and distinguish MIDI addresses from native identifiers before independently implementing controls |
| CSI QML | Symbolic controls, software behavior and feedback dependencies | Transport, numeric report fields, initialization and output messages |
| Generic MIDI Controller Manager | User-authored MIDI mappings are possible | A device-specific preset and any required mode, initialization or feedback |

Read-only inventory of the installed **Traktor Pro 4.4.1 build 22**, reviewed on
2026-10-12:

- `Factory Settings/4.4.1/Default Settings/Controller` and `AutoConfig` contain
  43 `.tsi` files: 23 NI, 12 Pioneer, 4 AlphaTheta, 2 Reloop, 1 Rane and 1 Volta XR.
  These counts are files, not distinct supported models; some repeat devices or
  represent different deck assignments, sequencer layouts and MIDI outputs.
- `Resources/qml/CSI` has nine NI device directories: MX2, X1MK3, Z1MK2,
  S2MK3, S3, S4MK3, D2, S5 and S8. Three other device directories are CDJ3000,
  XDJ1000MK2 and XDJ700. Shared `Common` helpers are excluded from this count.
- X1's `Kontrol X1 - AB.tsi`, F1's `Kontrol F1 - AB Track Deck.tsi` and
  `DDJ-FLX4.tsi` store `DeviceIO.Config.Controller` as a base64 value. The
  inspected X1 value decodes to a 68,496-byte binary container starting with
  `DIOM`, not an XML MIDI-address table. Its fields have not been decoded into
  a verified message map. File existence alone must not generate a preset.

The subsequent [read-only source audit](research/ni/traktor-4.4.1-audit.json)
checks all 23 NI `.tsi` files, nine NI CSI directories and the executable's
embedded output configuration. The independently authored
[inspector](research/ni/README.md) decodes only device/control descriptors and
audio channel facts. Native X1 definitions contain 120 input / 88 output
descriptors; F1 Track Deck has 42 / 271, S2 User 81 / 52 and S4 User 128 / 108.
These include alternate modes, not just physical controls. They establish
symbolic native definitions, not MIDI addresses or USB report offsets. S2/S4
User class names do not establish which hardware generation uses a packet layout.
One X1 Presets file contains settings without a controller definition.

This inventory provides a finite research backlog for the installed version,
not an exhaustive list of every controller Traktor can use. Generic MIDI support
also permits user mappings for devices outside the bundled list, as described in
the [Controller Manager manual](https://docs.native-instruments.com/ni-tech-manuals/traktor-pro-manual/en/configuring-midi-controller-for-controlling-traktor).
Basic MIDI control and complete native integration are separate coverage goals.
Full native coverage cannot be claimed from `.tsi` or QML alone, or without
checking per-model packet facts, supported mixless actions and physical behavior.
Screen, motor and Pattern Player parity remains outside this phase's scope.

Vendor material is inspected for factual interface information. mixless does not
ship Traktor scripts, translate its mapping logic, load proprietary NI libraries
or depend on a running Traktor process. Each implemented device retains its own
source and replay evidence.

## Official documentation refresh

On 2026-10-13, the official MIDI-mode and Controller Editor articles were
rechecked for the DJ-controller expansion. [MIDI mode](https://support.native-instruments.com/support/solutions/articles/69000880031-native-instruments-switching-your-controller-to-midi-mode)
and [custom template support](https://support.native-instruments.com/support/solutions/articles/69000879868-native-instruments-what-is-controller-editor-)
establish interfaces and user assignments, not a fixed per-control numeric table.
The reviewed MX2, X1 MK3 and S4 MK3 documentation did not supply a complete numeric
message map for a new preset. Existing F1 and original X1 template profiles are
retained; no new NI generation is inferred from them. Standalone mixers are
excluded from this expansion; the older Z1/Z2 findings below remain research.

## Current coverage

| Model | MIDI mode | Included mapping | Native protocol |
| --- | --- | --- | --- |
| MX2 | Both Shift buttons | No message map; Learn available; Master 1/2 and Cue 3/4 recommendation | NHL device layer identified; reports and initialization unresolved |
| X1 MK3 | Shift + mode/menu | No confirmed message map | QML behavior available; wire protocol unresolved |
| Z1 MK2 | Mode + menu | No confirmed message map | QML behavior available; wire protocol unresolved |
| X1 MK1 | Shift + Hotcue | SSL 2.2 template, Basic page | Traktor X1 symbolic descriptors decoded; packets unresolved |
| X1 MK2 | Shift + both Load buttons | No reviewed template/message map | Unresolved |
| F1 | Shift + Browse | FL Studio template, Basic page | Traktor F1 User symbolic descriptors decoded; packets unresolved |
| Z1 MK1 | Mode + both Cue buttons | No reviewed template/message map | Unresolved |
| Z2 | Shift + Settings | No reviewed template/message map | Unresolved |
| S2 MK1 / MK2 | Both Shift buttons | No reviewed template/message map | S2 User symbolic definition available; generation and packets unresolved |
| S2 MK3 / S3 | Hold left FLX during USB connection | No confirmed message map; latest firmware required | QML behavior available; wire protocol unresolved |
| S4 MK1 | Shift + Browse | No reviewed template/message map | S4 User symbolic definition available; generation and packets unresolved |
| S4 MK2 | Shift + Preview | No reviewed template/message map | S4 User symbolic definition available; generation and packets unresolved |
| S4 MK3 | Not listed by NI | No MIDI claim | QML behavior available; reports, display and motor protocol unresolved |
| D2 | Shift + Back | Installed templates reviewed; no usable fixed message list established | QML behavior available; wire protocol unresolved |
| S5 / S8 | Shift + right Back | No reviewed template/message map | QML behavior available; wire protocol unresolved |

The [NI MIDI-mode table](https://support.native-instruments.com/support/solutions/articles/69000880031-native-instruments-switching-your-controller-to-midi-mode)
is the source for switches and the S2 MK3/S3 firmware requirement. It does not
specify a minimum firmware for the other listed modes. No firmware version was
observed here. Neither an unknown device name nor a shared manufacturer name
automatically applies a preset.

## Included template-specific profiles

The two presets use numeric addresses from the **unmodified Controller Editor
2.8.2 templates named below**, not inferred factory addresses. Install/load the
named NI template yourself, select its Basic page, enter MIDI mode and explicitly
apply the mixless preset. The original templates are not redistributed or imported
by mixless. Changing their addresses or page changes the required mapping.

- **F1 (FL Studio template):** sixteen pads independently assigned to two decks'
  Play, Sync, PFL, Loop, Key Lock, Vinyl, Slip and Reverse. Four knobs adjust A
  stem gains; four faders adjust B stem gains. Browse/load and Quantize are mapped.
  Pad brightness uses the template's explicit brightness CC outputs. Hue,
  saturation, display and shifted page are omitted. No temporary-cue holds are
  assigned to pages whose release behavior has not been captured.
- **X1 MK1 (SSL 2.2 template):** FX mix, EQ, FX enable, transport, PFL, loop,
  beat jumps, browsing and loading. Both browse encoders send CC 16 in this
  template; there is one shared browse assignment. Loop-size encoders, shifted
  page, temporary cue and output feedback are omitted. MK2/MK3 are not covered.

The assignments in `src/catalogue/native_instruments.rs` are authored for mixless.
The template labels and other applications' command mappings were not translated.
The installed Controller Editor English manual, pp. 343 and 345, documents
relative offset as 65/63 and relative complement as 1/127; these determine the
F1 and X1 encoder decoders. Numeric facts were inspected from the vendor
installation; source software is
available through [NI Drivers & Other Files](https://support.native-instruments.com/support/solutions/articles/69000882238-native-instruments-drivers-other-files).
Synthetic fixtures retain independently written actions, button-edge expectations,
encoder direction, wrong-page omissions and F1 brightness deduplication. Both
presets remain `protocol`; neither is hardware verified.

## MX2 native interface findings

Traktor Pro 4.4.1 build 22 includes five MX2 QML files in
`Contents/Resources/qml/CSI/MX2`. They bind symbolic device controls to software
parameters. Jog rotation, speed and touch are separate values; touch is enabled
in Turntable mode. Pad modes are software state. Cue/Master mixing and headphone
volume are software parameters. Native colors and bottom lights have independent
state. These findings agree with the [MX2 user guide](https://docs.native-instruments.com/ni-tech-manuals/traktor-mx2-user-guide/en/customizing-your-mx2).

The installed executable identifies `NHL2::Traktor::MX2::Device`,
`MX2PortDescriptor`, `MX2SubscriptionListener` and USB device handles. That
establishes an NHL device abstraction, **not** a HID report specification.
No usable report layout, endpoint list, initialization bytes or MIDI address
table was established in this review. No NI native adapter or speculative USB
transport is shipped. Do not load/call vendor binaries or treat symbolic QML
paths as MIDI addresses. A later implementation needs documented packet facts
or independently recorded traces before adding endpoint permissions and fixtures.

The executable's embedded audio configuration specifies zero-based Master
channels 0/1, Monitor 2/3 and microphone input 0/1. Only the output recommendation
is implemented; [NI's setup guide](https://blog.native-instruments.com/getting-started-traktor-mx2/)
also gives Master 1/2 and Headphone 3/4. The user reviews and applies routing.
Screens, motor control, microphone input and Pattern Player remain out of scope.

## NI controller audio recommendations

Traktor's embedded per-device internal-mixer XML confirms Master 1/2 and Cue 3/4
for each of these 13 exact source names. mixless offers those output pairs through
**Details → Audio routing…**, including when no controller preset is selected.
This edits the Audio settings draft; the user reviews device availability and
chooses Apply before any audio output changes.

| Source device name | Generation / model |
| --- | --- |
| Traktor MX2 | MX2 |
| Traktor Kontrol Z1 | Z1 original |
| Traktor Z1 MK2 | Z1 MK2 |
| Traktor Kontrol Z2 | Z2 |
| Traktor Kontrol S2 | S2 original |
| Traktor Kontrol S2 MK2 | S2 MK2 |
| Traktor Kontrol S2 MK3 | S2 MK3 |
| Traktor Kontrol S3 | S3 |
| Traktor Kontrol S4 | S4 original |
| Traktor Kontrol S4 MK2 | S4 MK2 |
| Traktor Kontrol S4 MK3 | S4 MK3 |
| Traktor Kontrol S5 | S5 |
| Traktor Kontrol S8 | S8 |

Each device's channels were checked independently and retained in the source
audit. No recommendation is inferred for unknown generations, X1, F1 or D2.
Name matching is exact; actual OS names, legacy drivers, output availability and
physical routing still require later verification. This does not add input or
feedback mappings for these 13 devices. Sample rates, buffers, hardware mixer
settings and microphone inputs are not applied from Traktor's configuration.

## Source fingerprints

SHA-256 of the inspected vendor files; no vendor source or binaries are bundled.

| Installed file | SHA-256 |
| --- | --- |
| Traktor Pro 4 executable (4.4.1 build 22) | `cdc051995be78f6cfa88e3da1ccc7eee0bd6a982e27351177140bc184fe3ecb6` |
| MX2.qml | `25d20d40d4cd3448e4f45cf028001d52fba56443f8df25164079fb29ab101409` |
| FL Studio.nckf1 | `c6597c267ec8f4caf0f3fff186d030636fb65b95d29acdc2affa6a1aea663a94` |
| Serato Scratch Live 2.2.nckx1 | `a65088b447f88d4bc1640b0aa838eb4df36f9cf5b81b50feb5a07474c858ad55` |

## Offline validation and later capture

`examples/ni-f1` and `examples/ni-x1` include portable presets, independently
written fixtures and pinned traces. Replay uses no hardware:

```sh
target/debug/mixless-controller replay crates/mixless-controllers/examples/ni-f1/preset.json crates/mixless-controllers/examples/ni-f1/trace.json --check
target/debug/mixless-controller replay crates/mixless-controllers/examples/ni-x1/preset.json crates/mixless-controllers/examples/ni-x1/trace.json --check
```

For a later, explicitly selected capture session, `capture-input` opens only the
chosen input, applies no preset and sends no initialization, feedback or feature
requests. HID report-descriptor metadata is read to normalize report IDs. Opening
an input can affect OS ownership; close other software as needed. It cannot
capture an undocumented vendor USB interface that is not exposed through HID.

```sh
target/debug/mixless-controller capture-input midi '<exact input ID>' /tmp/mx2-input.json 30 'Traktor MX2' 'unknown' 'macOS version'
```

Type a short control label and Enter to add an operation marker. Captures are
bounded to 4096 frames and 8 MiB of stored frame data; queue loss or oversize
messages mark them truncated. Local IDs/paths are not exported; raw bytes still
need review before sharing. An input observation is deliberately a different
format from `ReportTrace`: it is not accepted by replay or pin-trace and makes
no claim about control correctness, feedback, firmware compatibility or latency.
