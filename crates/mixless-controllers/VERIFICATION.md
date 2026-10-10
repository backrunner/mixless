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
