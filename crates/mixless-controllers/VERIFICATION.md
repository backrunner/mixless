# Local controller verification — 2026-10-09

For the subsequent community package, sandboxed adapter and recording/replay
changes, see [Community adapter verification](COMMUNITY_VERIFICATION.md).

Implementation and validation in the macOS development workspace. No DJ
controller was physically available. This record is not hardware certification.

## Automated checks

- `cargo test --locked --workspace --exclude mixless-desktop`: **401 passed,
  22 ignored**. Includes native-preset CRUD, rollback, migration, portable export,
  per-device isolation, MIDI precision/framing, HID fields/counters, layer
  releases and soft takeover; engine tests cover channel overlap, independent
  440 Hz Master / 997 Hz Cue signals, unused-channel silence and missing Cue sink.
- `cargo test --locked -p mixless-desktop`: **111 passed, 6 ignored**. Includes
  hardware dispatch without UI ticks and browse/load/play ordering, along with
  the existing desktop, AutoMix and updater suite.
- `cargo test -p mixless-controllers virtual_ --locked -- --ignored --nocapture
  --test-threads=1`: **2 passed**. Actual CoreMIDI virtual ports test learning,
  discovery after startup, feedback deduplication, two simultaneous inputs,
  learning scoped to one device while the other continues, and release on
  unplug. Temporary test configuration is isolated from the user's library.
- Clippy with `-D clippy::correctness`: no errors. Style warnings and existing
  dependency/deprecation warnings remain; this is not a warning-free build.
- Desktop debug build and native `.app` packaging succeeded.

## Latency

The virtual-port test uses the full DDJ-400 declaration plus independently
invented test controls and dispatches into the real offline audio engine. It
sends 500 controls at 4 ms intervals while never calling the UI drain/poll path.
The latest run measured callback receipt to engine command dispatch:

| percentile | microseconds |
| --- | ---: |
| p50 | 85 |
| p95 | 265 |
| p99 | 754 |

The p99 assertion is below 5000 microseconds. These figures exclude device scan
latency, USB/Bluetooth transmission and the audio output buffer. The test
engine's 256-frame/48 kHz configuration corresponds to **5.33 ms** of audio
buffer duration; it is not a measured physical output or round-trip latency.
HID timestamping starts when the OS report is read, not at the physical control.

## Evidence still needed

At the initial verification, the native UI automation service failed before
application access, including after resetting it: `Sky Computer Use native pipe startup failed`. Therefore
native clicking, visual layout and current Retina screenshots were not verified
at that stage. The later native UI verification below closes this gap using
macOS Accessibility and CGEvent input. Historical screenshot metadata is retained.

No real-device HID input/output/feature report round trip, DJ-controller audio
output, sleep/wake, firmware variants, occupied-device behavior or physical jog
calibration is claimed. No device is labeled hardware tested. CoreMIDI's
persistent endpoint IDs and HID serial/path/interface identities are used for
reconnection; a changed/ambiguous identity remains unassigned until explicitly
selected. CoreAudio UIDs are persisted, and ambiguous duplicate output names are
rejected by the current CPAL adapter instead of selecting an arbitrary device.

Independent protocol references and profile coverage limits are in
[SOURCES.md](SOURCES.md). The app retains MPL-2.0; provenance does not constitute
a full legal audit of all transitive dependencies.
## Catalogue expansion — 2026-10-10

The catalogue now contains 12 independently authored protocol profiles. This
update adds DDJ-200, DDJ-SB, DDJ-SB2, DDJ-SB3, DDJ-RB, DDJ-REV1, DDJ-WeGO3,
DJControl Inpulse 200, DJControl Inpulse 300 and Numark DJ2GO. Generations are
matched explicitly: original Inpulse and DJ2GO profiles do not advertise
compatibility with MK2/MK3, DJ2GO2 or Touch variants. Every profile remains at
`Protocol` verification status. Manufacturer references and exclusions are
recorded in SOURCES.md and portable profile notes shown in settings.

Checks for this expansion (separate from the earlier workspace run above):

- `cargo test --locked -p mixless-controllers -p mixless-midi`: **31 passed,
  2 ignored** (27 controller tests and 4 MIDI tests). Coverage includes every
  profile's validation/export round trip, both decks' transport and momentary
  Cue edges, model isolation, differing pad/mixer channels, initialization,
  negative jog deltas and device-scoped Shift cleanup.
- `cargo test --locked -p mixless-controllers virtual_ -- --ignored --nocapture
  --test-threads=1`: **2 passed**. CoreMIDI callback-to-handler latency:
  **p50 121 µs, p95 157 µs, p99 443 µs**. The same exclusions and audio-buffer
  distinction described above apply; this is not a physical latency measurement.
- `cargo check --locked -p mixless-desktop`: passed. Existing engine deprecation
  and dependency future-compatibility warnings remain.
- `git diff --check`: passed.

A parallel test run exposed a native macOS HID enumeration crash. The macOS
backend retains its global IOKit manager on the initializing thread's CFRunLoop;
initializing it from an ephemeral caller could outlive that thread. Discovery
and device opening now run through a persistent process-wide worker. A regression
test enumerates from four waves of eight short-lived concurrent callers. It and
the complete controller/MIDI test run passed after the change. This verifies
native enumeration lifecycle, not controller-specific HID reports.

Shift changes now clear all inactive-layer edge states for that device, including
non-held buttons, and release held Cue/Jog actions. Preset Cue controls use
momentary hold/release semantics. These behaviors are covered by synthetic
fixtures written for mixless, not by imported mapping data.

Physical validation and calibration are deferred. The native application bundle
built on 2026-10-09 predates this expansion; it is not evidence for the 12-profile
catalogue. No new installed-build or native visual validation is claimed here.


## Native controller UI verification — 2026-10-10

Tested the actual macOS development application using Accessibility window
control, CGEvent mouse/keyboard input and native file dialogs. This is separate
from the GPUI regression tests. The app used an isolated temporary data directory
with offline analysis/audio settings; the user's library and settings were not
changed. No new installed or packaged artifact is claimed.

Native interactions verified:

- All 12 built-in presets appear in the selector. Offline selection does not
  assign a device; Apply is disabled without one. Built-in presets remain
  read-only and editing creates a user copy.
- New, duplicate, rename, delete cancellation/confirmation and restart
  persistence. A renamed imported preset survived application restart.
- Native export/open dialogs, portable export (82 DDJ-400 bindings, no local
  device ID), import preview with device requirements and conflict counts,
  cancellation without library changes and acceptance as a separate user preset.
- Unsupported-version rejection without configuration changes, then a valid
  import retry with no stale error. Invalid JSON editor rollback is additionally
  covered by the GPUI test below.
- Controller diagram, wrapped coverage notes, mapping table, selection highlight
  and editor Save/Cancel at 940 × 820 and the minimum 720 × 540 window. Audio I/O
  routing controls were also visually inspected at the minimum size; this was
  a layout check, not a physical audio-output test.
- A temporary CoreMIDI source/destination named DDJ-400 was created while the
  application ran. It appeared automatically, offered the matching preset, and
  remained unassigned until Apply. A synthetic `90 0B 7F` input appeared in the
  report monitor. Removing the ports showed Offline while retaining the saved
  assignment. The virtual endpoints were disposed after the check.

Issues found and fixed through these interactions:

- Absolute-positioned diagram controls collapsed into a zero-width flex item;
  the diagram now receives a definite width based on the viewport.
- Small windows clipped editor actions. Modal height and editor scrolling now
  respect the viewport, and the backdrop prevents clicks on background controls.
- Text selections were invisible; the editor now paints their highlight.
- A failed import left an error visible during the next successful preview.
- Device-menu scrolling also scrolled the underlying page. The popup now blocks
  underlying input; MIDI devices appear first and HID interfaces include their
  VID/PID, interface and usage to distinguish identical names.
- Mapping sources displayed long Rust debug values. Compact source labels retain
  full details in a tooltip, and unavailable offline learning/assignment actions
  remain disabled.

Verification after the fixes:

- Four GPUI tests exercise the actual render tree and click/keyboard/scroll
  handling: diagram geometry at three sizes (also 1200 × 900), modal isolation
  and offline selection, import retry/rollback, and device menu ordering/scroll.
- `cargo test --locked -p mixless-desktop`: **115 passed, 6 ignored**.
- `cargo build --locked -p mixless-desktop` with `MIXLESS_CHANNEL=dev`: passed.
- `cargo fmt -p mixless-desktop -- --check` and `git diff --check`: passed.
  Workspace-wide formatting still reports an existing module-order difference
  in `crates/mixless-protocol/src/lib.rs`; it is outside this UI change.
  Existing engine deprecation and dependency future-compatibility warnings remain.

Current native 2× captures are
[Controllers](../../assets/screenshots/controllers.png) and
[minimum-size editor](../../assets/screenshots/controller-editor.png).
Their individual capture times, dimensions and exact development build identity
are recorded in [capture.json](../../assets/screenshots/capture.json). Older
screenshots retain their original metadata.

These checks establish native UI behavior and virtual MIDI integration only.
No physical controller, HID report round trip, firmware variant, jog calibration
or controller audio routing was validated; hardware status remains unchanged.

## Pre-commit verification — 2026-10-10

Rechecked the combined controller, audio-routing and settings changes on macOS
27.0.1 (26A434):

- `cargo test --locked --workspace --exclude mixless-desktop --quiet`:
  **405 passed, 22 ignored**.
- `cargo test --locked -p mixless-desktop --quiet`:
  **123 passed, 6 ignored**.
- `cargo test --locked -p mixless-controllers virtual_ -- --ignored --nocapture
  --test-threads=1`: **2 passed**. CoreMIDI callback-to-handler latency was
  **p50 124 µs, p95 164 µs, p99 394 µs**. These are virtual-device dispatch
  measurements, excluding physical transport and the audio buffer.
- `cargo test --locked --release -p mixless-engine callback_budget -- --ignored
  --nocapture --test-threads=1`: **5 passed**, run without concurrent builds or
  tests. The 48 kHz / 256-frame callback measured **p99 0.184 ms** against a
  **5.333 ms** buffer. The two-deck stems, pitch, FX, recording and cue case
  measured **p99 0.983 ms** against a **2.667 ms** buffer.
- Formatting passed for `mixless-controllers`, `mixless-midi`, `mixless-engine`
  and `mixless-desktop`.
- `./dev.sh --build`: passed and refreshed `target/app/Mixless.app`. This checks
  development packaging, not installation, release signing or notarization.
- Site content validation: **24 pages, 122 search records, no errors or
  warnings**.

The latest settings render and interaction evidence is recorded separately in
[preferences/VERIFICATION.md](../../apps/desktop/src/preferences/VERIFICATION.md).
No additional native pointer interactions were attempted during this final
check because the desktop was locked. No physical controller was available;
hardware validation status remains unchanged. Existing engine deprecation and
dependency future-compatibility warnings remain.

## NI offline development — 2026-10-12

Host: macOS 27.0.1 (26A434), Apple Silicon. Audio tests used synthetic PCM
and offline output buffers; no NI device was opened or captured and no
physical audio/listening or firmware verification was performed.

- `cargo test --locked --workspace --features mixless-controllers/tools`: 564
  passed, 30 ignored. Includes the controller tools and desktop tests.
- Final desktop review: 125 passed, 6 ignored, including full-address editing,
  invalid CC14 pairing, calibration and settings compatibility. The production
  render tree checks the expanded editor at 940 × 820 and 720 × 540 with an
  offline UI identity, plus the fixed Done footer. The controller hub stays disabled.
- Release callback budgets: 5 passed with no concurrent build/test workload.
- C SDK example: 8 frames and 8 assertions passed. NI F1/X1 pinned synthetic
  traces pass and remain separate from hardware captures.
- `./dev.sh --build`, workspace formatting, `git diff --check`, bilingual
  strict content checks, website type checks and static build passed.

Native capture was attempted with an isolated, ad hoc signed preview bundle,
offline audio and disabled real controller inputs. WindowServer returned
`cgWindowNotFound`; `IOConsoleLocked=Yes` confirmed the desktop was locked.
No native screenshot or visual-presentation claim is recorded. The attempted
preview process was stopped after checking PID, UID, start time and command;
its exit was verified. Native pixel review still needs an unlocked desktop.

The per-model implementation/gap matrix and original source fingerprints are
in [NI_COMPATIBILITY.md](NI_COMPATIBILITY.md). MX2/X1 MK3/Z1 MK2 and NI
native packet formats remain unresolved; no placeholder compatibility preset
or native adapter is published. No remote CI, deployment or device certification
is claimed by this local validation.

Final development bundle: version `0.1.0-beta.4`, channel `dev`,
revision `v0.1.0-beta.4-5-g3da6ea1-dirty`, built `2026-10-12T05:58:20Z`.
Source executable SHA-256: `e21b0af2cb54d5d76d5bbaba83ce8912ffa240401c63014d00a22ced03e19887`.

## NI Traktor source audit and audio expansion — 2026-10-12

Continued with NI's own devices only. The read-only inspector independently
parses source descriptor containers and embedded output-channel XML, without
decoding Traktor command assignments or running vendor software. The retained
audit was reproduced exactly from the installed files: 23 NI TSI files (22 with
controller definitions), nine NI CSI directories with 58 QML files, and 13
controller audio configurations. Hashes match the inspected vendor installation.

The audio helper now covers those 13 exact source model names. A regression
compares every output pair with the retained source audit and rejects unknown
generations, appended names and X1/F1/D2. No new empty preset or NI native adapter
was added; X1/F1/S2/S4 descriptors remain symbolic, with packet layouts unresolved.

Validation after the expansion:

- Workspace with controller tools: **567 passed, 30 ignored** (31 suites).
- Inspector: **9 synthetic tests passed**, covering malformed XML/base64,
  truncation, chunk boundaries/counts, multiple definitions, skipped private
  fields/mapping assignments, bounded reads, conflicting audio duplicates,
  invalid channel pairs and exclusion of third-party folders.
- Desktop: **127 passed, 6 ignored**, included in the workspace run. Two new
  production render-tree tests verify routing without a control preset at
  940 × 820 / 720 × 540, a click that changes only the Audio draft, and no
  recommendation for an unknown NI generation. The hub remains disabled and
  all device identities in these UI tests are fictional.
- C SDK example: **8 frames, 8 assertions passed**.
- Desktop bundle build, Rust formatting and diff checks passed. Bilingual
  strict content checks report 24 pages / 126 records with no errors or warnings;
  website type checks and static build also passed.

The host still reports `IOConsoleLocked=Yes`; no new native pixel capture was
performed. The checks establish source inspection and software behavior, not
device firmware, physical audio routing or native USB compatibility. No real NI
device was opened, operated or captured. No audio callback code changed in this
expansion; the five callback budgets above belong to the earlier CueMix change.

Development bundle after this expansion: `0.1.0-beta.4`, channel `dev`,
revision `v0.1.0-beta.4-5-g3da6ea1-dirty`, built `2026-10-12T07:34:53Z`.
Bundled executable SHA-256:
`f3872f11309a02225234f4c9fbebc16d5b644ed7b4c7d13ad75f6a1cf6c792c3`.
Build identity was read through `--build-info`, which exits before AppKit/audio
startup. The application was not launched for native presentation in this pass.

## Learn and calibration regression checks — 2026-10-12

Reviewed the combined pending NI changes and corrected fractional relative
steps, shared normal/shortcut Learn arming, CC14 disconnect/arm epochs, manual
address-edit cancellation, legacy input diagnostics and calibration validation.
NI template buttons were also checked as gate controls with 0/127 values;
no template/application command
assignment logic was imported.

Final local checks:

- Workspace including controller tools: **572 passed, 31 ignored**.
- Explicit shortcut-Learn GPUI/CoreMIDI test: **1 passed**, using one selected
  virtual input only. A single CC half is rejected, the full pair saves the
  correct address, and existing calibration survives. The virtual source was
  closed and its hub disabled at the end. No physical input/output was opened.
- Controller/source inspector: **9 synthetic tests passed**.
- C SDK example: **8 frames / 8 assertions passed**.
- Release audio callback budgets: **5 passed**, with no concurrent build/test
  workload. Rust formatting, diff checks and desktop bundle build passed.

The new CI inspector step has not run remotely; these results are
local. The rebuilt bundle contains no QML/TSI/NI template files and no NI-named
native artifacts, and its executable's dynamic dependency list contains no
NI/Traktor/NHL library. This verifies the checked artifact and explicit
dependencies, not the origin or legal status of every possible code fragment.
The Mac remains locked, so native pixel review is still incomplete; synthetic
render-tree checks are not claimed as physical presentation or NI acceptance.

Latest bundle identity: `0.1.0-beta.4`, channel `dev`, revision
`v0.1.0-beta.4-5-g3da6ea1-dirty`, built `2026-10-12T08:39:27Z`.
Bundled executable SHA-256:
`186c2c4e1e2011ce01c58e06038330a267f8ab589c474cd41ff3a1930b915fc7`.
