# Mixless

Local AI Mixing / AI DJ desktop app. Dual decks, offline analysis, playlist automix.

- **Audio**: Rust realtime graph (`mixless-engine` + CoreAudio)
- **UI**: [GPUI](https://github.com/zed-industries/gpui) (pure Rust, GPU-rendered, no web stack)
- **License**: Apache-2.0 (optional closed-source acquire plugin is not in this tree)

v1 targets **macOS** only. Design lives in [`.agents/`](.agents/README.md).

## Workspace

```
apps/desktop/          GPUI app (mixless-desktop bin "mixless")
crates/mixless-protocol
crates/mixless-engine
crates/mixless-library
crates/mixless-analyze
crates/mixless-mixplan
crates/mixless-spotify
crates/mixless-acquire
crates/mixless-acquire-yt
```

`apps/desktop` must not contain DSP; it only hosts engine/library state and paints UI.

## Develop

```bash
# Build incrementally and launch the desktop development version
./dev.sh

# Check prerequisites, or build without launching
./dev.sh --check
./dev.sh --build

# Rust crates
cargo test --workspace --exclude mixless-desktop
```

Requires a current stable Rust toolchain and full Xcode (the `metal` shader
compiler is not part of the standalone Command Line Tools). `dev.sh` locates
the project even when called from another directory and automatically selects
an Xcode installation with Metal support, including Xcode-beta. An explicit
`DEVELOPER_DIR` takes precedence. Press Ctrl+C to stop; rerun after code changes
to rebuild and replace the previous instance from this checkout. Click MIXLESS / ABOUT to see the version, Git revision and UTC build time.
The main window keeps build metadata out of the workspace. The first
build optimizes the audio engine, GPUI and waveform tessellation for development
and may take longer. The script uses the normal local app library and does not provide
automatic file watching.

The UI mirrors `EngineSnapshot` from the engine's atomics at display VSync
while audio is active, and repaints on demand while idle — the playhead
is never integrated on the UI side. Keymap: Space play/pause, F xfader center,
S sync, QWER/UIOP hot cues 1–4.

The library fills the remaining window height. Use **+ Files** or **+ Folder**
for local imports (folders include subfolders; duplicate paths are skipped),
or **Spotify / Details** for playlist import and progress details. Native file
selection is asynchronous and folder scanning runs on a worker. Drag a library
track onto either deck to load it; the receiving deck highlights during the drag.
Loading a track manually takes over from Automix.

The waveform uses a symmetric peak envelope colored by low/mid/high spectral
energy, with a darker RMS body and a fixed playhead. Beat and downbeat markers
come from stored analysis; tracks without a measured grid show no invented bar
lines. Tempo changes preserve the source-time grid. This is an independent
renderer inspired by DJ workstations, not a pixel-identical reproduction.

## Automix

The `AUTO` control plans one outgoing/incoming pair at a time. It loads the
next local track in the idle deck, resolves the next transition from the
offline analyses and compiles the envelope into the audio thread. `PAUSE`,
`RESUME`, `SKIP` and direct fader/EQ/filter edits are available while a plan is
running; editing a lane takes over that lane only. User mix-in/mix-out cues are
hard constraints, hot cues are soft anchors, and an impossible cue window is
reported instead of bypassed.

For a device-free preview from two files:

```bash
cargo run -p mixless-engine --example automix -- outgoing.wav incoming.wav preview.wav 128 126
```

The planner supports local tempo maps, Camelot compatibility, energy/vocal/kick
scoring, 1:1 and 2:1 bar maps, `energy_hold` offset inheritance, and the nine
catalog envelopes described in `.agents/05-ai-mixing.md`. Tracks with only the
current quick BPM/key analysis use the constrained fallback envelope until
structure and per-bar analysis is available.

Implementation details, verification and current limitations are in
[AUTOMIX.md](AUTOMIX.md).

Audio DSP behavior, regression checks and remaining limitations are documented
in [AUDIO_DSP.md](AUDIO_DSP.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for checks and Conventional Commits.
See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for bundled code licenses.
