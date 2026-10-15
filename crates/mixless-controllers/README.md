# Community controller development

Use a declarative preset for fixed MIDI messages or HID report fields. Use a Wasm
adapter when a HID protocol needs a handshake, checksum, sequence tracking,
feature requests or timers. Both are portable JSON files imported through
**Controllers → Manage → Import**. Select one or multiple files. The user
reviews each file and its device/adapter details, imports separate copies, then
explicitly applies a preset to a device. Invalid files remain visible and are
skipped; valid files are saved in one transaction, with no partial save on failure.
Batch review accepts up to 128 files and 64 MiB of selected data. Duplicate names
and IDs create separate copies; active assignments are preserved.

The host does not fetch or execute Mixxx code, scripts, mappings or converted
presets. Write adapters from manufacturer protocols or independently collected
reports. Record the document URL, revision and section in `provenance`. A HID
descriptor identifies report layout, not the musical meaning of a control.

## Tools

Build the development CLI (it is separate from the desktop executable):

```sh
cargo build --locked -p mixless-controllers --features tools --bin mixless-controller
target/debug/mixless-controller targets
target/debug/mixless-controller schema
target/debug/mixless-controller validate preset.json
```

Use [controller-preset.schema.json](sdk/controller-preset.schema.json) in your
editor. It describes both forms and enumerates the actual host targets. Runtime
validation additionally checks endpoint references, calibration, capability
permissions, module hashes, API compatibility and Wasm resources. Do not add
`$schema` to a preset: presets reject unknown fields. Configure the schema in
your editor's file association instead.

The [declarative example](examples/declarative-hid/preset.json) has fixed button
and fader inputs and an application-state LED. Its independently written
[fixture](examples/declarative-hid/fixture.json) checks edges, normalization,
feedback deduplication and disconnect release:

```sh
target/debug/mixless-controller pin-trace \
  crates/mixless-controllers/examples/declarative-hid/preset.json \
  crates/mixless-controllers/examples/declarative-hid/fixture.json /tmp/fixed.trace.json
target/debug/mixless-controller replay \
  crates/mixless-controllers/examples/declarative-hid/preset.json /tmp/fixed.trace.json --check
```

Both examples use a fictional VID/PID and invented reports. They are development
fixtures, not compatibility claims for any physical product.

The controller list shows HID interfaces only when their VID/PID and any
declared interface, usage page and usage match an installed controller preset.
Unknown HID devices are hidden. Keyboards, pointers, headsets and security keys
do not become controller candidates just because the OS exposes them through
HID. Import the corresponding HID preset before selecting its device; applying
it remains explicit. MIDI input discovery is unchanged.

## Build an adapter

[mixless_controller.h](sdk/mixless_controller.h) defines ABI 1. The
[C example](examples/synthetic-hid/adapter.c) demonstrates a readiness handshake,
checksum, sequence deduplication, a wrapping platter counter, a held control,
feedback and bounded retries. It needs no libc or WASI.

With LLVM clang (including its wasm32 target) and wasm-ld installed:

```sh
clang --target=wasm32 -O2 -nostdlib -Wl,--no-entry -Wl,--export-memory \
  -Wl,--initial-memory=131072 -Wl,--max-memory=2097152 \
  -o /tmp/adapter.wasm crates/mixless-controllers/examples/synthetic-hid/adapter.c
target/debug/mixless-controller pack \
  crates/mixless-controllers/examples/synthetic-hid/preset.template.json \
  /tmp/adapter.wasm /tmp/controller.json
target/debug/mixless-controller pin-trace /tmp/controller.json \
  crates/mixless-controllers/examples/synthetic-hid/fixture.json /tmp/controller.trace.json
target/debug/mixless-controller replay /tmp/controller.json /tmp/controller.trace.json --check
```

`pack` also accepts `.wat` text for small hand-authored adapters. Production
imports accept only an embedded Wasm binary. `pack` embeds it as base64, records
its BLAKE3 digest and validates it. It never executes compiler commands from a
package. Output files are created atomically without overwriting an existing file.

`python3 crates/mixless-controllers/sdk/verify-example.py` compiles and checks the
C example in temporary files. It checks that clang supports wasm32 and can find
Homebrew LLVM (`brew install llvm lld`) when the clang on PATH does not. Set
`CLANG` and `WASM_LD` to select tools explicitly. The script checks that wasm-ld
can run and searches PATH, Homebrew lld and the rustup toolchain. Repository CI
uses explicit paths to LLVM clang and lld.

Other languages can target the same ABI. A script written for another DJ app is
not compatible with this interface and is not an accepted implementation source.

## Share and update

`community` records a stable `package_id`, semantic `package_version`, minimum
host version, author, license identifier, full license text and HTTPS source URL.
Include original source, fixture files and protocol references with releases.
`tested_on` records publisher-supplied model, firmware, OS and evidence links;
the host does not treat those claims as its own hardware validation.

The imported JSON is self-contained. Local device IDs and paths live in the
separate assignment store and are never exported. Import always creates a new
local preset ID. There is no automatic downloading or replacement of a running
preset. To update, import the new version and explicitly apply it; retain the old
copy to roll back. A BLAKE3 content fingerprint identifies the full portable
content, excluding only the installation ID and host verification label.

Start community distribution with versioned repository releases containing the
JSON, source, license and reviewed fixtures. No marketplace or network package
installer is required. Authors choose a license they have the rights to grant;
metadata and checksums are not publisher authentication or a license clearance.

## Capture and replay

```sh
target/debug/mixless-controller devices
# For a HID controller, supply its preset to identify its interfaces:
target/debug/mixless-controller devices preset.json
```

Create a local `ports.json` mapping every logical endpoint to one exact device ID
printed by that command, for example `{"main":"<exact ID>"}`. Do not publish
this file. Then use a development session with the device available:

```sh
target/debug/mixless-controller record preset.json ports.json capture.json 30 "firmware-version" "macOS-version"
target/debug/mixless-controller replay preset.json capture.json
```

`devices [PRESET]` only enumerates; the optional preset is installed in an
isolated temporary store for this invocation. It opens no HID interfaces and
sends no reports. **`record` opens the selected inputs, applies the
preset and sends its initialization/feedback reports.** It uses a temporary
configuration and starts no audio playback. Close the device in another app if
necessary. A capture starts at connection, records input and feature replies,
timer callbacks and redacted software state, and ends when the assignment is
removed. The CLI's software state is an idle engine; use fixtures to supply
playback states for feedback tests.

Captures contain no host library titles, track IDs or local audio-device IDs.
Raw protocol bytes can still include device-specific information; review them
before sharing. Recording is opt-in, memory-bounded and never performed on an
audio thread. The 4096-frame / 8 MiB capture limit marks truncated recordings as
incomplete. Replay rejects incomplete or mismatched captures.

Disabling controllers closes the recorded session. Lost input or adapter queue
events mark the capture incomplete, so a partial session cannot be mistaken for
a reproducible fixture.

Replay prints actions and output/feature requests without opening a device.
An observed capture is not a passing test: review the result against the
protocol, add `expected` effects to each frame (including empty arrays), and use
`--check`. `expected_error` supports deliberate failure cases. `pin-trace`
checks all expectations before writing a fixture with the current content
fingerprint. It does not silently bless observed output. Timers must have been
scheduled, timestamps must be ordered, and sessions must end in disconnect.

Report compatibility as **protocol implementation**, **simulation verified**,
or **hardware verified**. Replay cannot prove USB behavior, firmware variants,
physical latency or audio routing. Firmware and OS evidence are needed from
community members with the actual controller.

See [ADAPTER_API.md](ADAPTER_API.md) for the ABI and failure behavior.

## NI controllers and input-only observation

See [NI_COMPATIBILITY.md](NI_COMPATIBILITY.md) for template-specific F1/X1 MK1
profiles and the MX2, X1 MK3, Z1 MK2 and other NI protocol gaps.
`presets` lists built-in IDs; `export-builtin ID OUTPUT` exports a portable copy.

NI controller audio recommendations use Traktor's per-device internal-mixer
configuration for 13 exact model names. **Details → Audio routing…** proposes
Master 1/2 and Cue 3/4 for review, including without a control preset. Apply in
Audio I/O is still required. A recommendation is not controller compatibility.
The [read-only Traktor inspector](research/ni/README.md) can reproduce the NI
factory-source audit without device access or running vendor software.

`capture-input midi|hid DEVICE_ID OUTPUT SECONDS MODEL FIRMWARE OS` opens one
exact input without a preset or any output/feature requests. Type a control label
and Enter for an operation marker. It records observations rather than reviewed
replay fixtures. Do not use the existing `record` command when you need passive
input capture: `record` applies a preset and can send initialization/feedback.
No physical captures were performed for the NI offline release.

## Built-in DJ controllers

The catalogue contains 39 presets, including 34 Pioneer DJ / AlphaTheta
controllers. [Expansion sources and coverage](examples/alphatheta-ddj/README.md)
record per-model setup, omissions and 24 pinned replays. Built-ins remain
protocol-verified; physical firmware, calibration, lights and routing are pending.
The menu uses device-only names and natural model ordering; None stays first.
