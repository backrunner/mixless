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
to rebuild. The script uses the normal local app library and does not provide
automatic file watching.

The UI mirrors `EngineSnapshot` from the engine's atomics on a 60 Hz frame
budget while audio is active, and repaints on demand while idle — the playhead
is never integrated on the UI side. Keymap: Space play/pause, F xfader center,
S sync, QWER/UIOP hot cues 1–4.

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
