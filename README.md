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
to rebuild and replace the previous instance from this checkout. Choose **Mixless → About Mixless** from the macOS menu bar to see the version, Git revision and UTC build time.
The main window keeps build metadata out of the workspace. The first
build optimizes the audio engine, GPUI and waveform tessellation for development
and may take longer. The script uses the normal local app library and does not provide
automatic file watching.

The UI mirrors `EngineSnapshot` from the engine's atomics at display VSync
while audio is active, and repaints on demand while idle — the playhead
is never integrated on the UI side.

## App icon

The continuous-wave M, with a warm gradient and soft enamel shading, appears
in the Dock, header and About window. The [branding assets](assets/branding/README.md)
include the refined image master, an opaque 1024 px sRGB store PNG, rounded
macOS assets and ICNS, and the original vector artwork. Regenerate the local icon exports with
`swift scripts/generate-icons.swift`; after building, run
`python3 scripts/bundle-macos.py` to assemble a local unsigned `.app`.

## Preferences

Choose **Mixless > Preferences...** in the macOS menu bar or press **Cmd+,**.
The single preferences window contains General, Audio I/O and MIDI Mapping tabs.

- General saves waveform layout, FX visibility, quantize, key lock, vinyl/slip,
  crossfader curve and reverse. Changes also apply to the current session.
- Audio I/O selects the master and a separate headphone output, sample rate
  (up to 96 kHz), buffer size and headphone volume. **Apply audio** reopens the
  streams; unavailable devices or unsupported formats report an error and retain
  the prior configuration. Reconfiguration briefly interrupts playback.
- PFL uses a separate pre-fader headphone bus. It remains independent of the
  channel faders and master volume, including with different device sample rates.
  No headphone output disables PFL only; hot cues remain available.
- MIDI Mapping selects enabled input devices and supports CC/note mappings,
  learning, editing, deletion and JSON import/export. Learning captures the first
  signal and suppresses performance commands until saved or canceled. Jog uses
  two's-complement relative CC. Legacy maps without a device ID match any input.

Preferences and MIDI maps persist in `preferences.json` and `midi.json` beside
the library database. Disconnected MIDI devices can be refreshed and reconnected
with **Apply inputs**. Audio capture from microphones/Line In is not implemented.

## Keyboard shortcuts

Choose **Help → Keyboard Shortcuts** in the macOS menu bar, or press **? / F1**, to view shortcuts.
The highlighted deck receives selected-deck commands; **Tab** switches A/B.

| Keys | Action |
| --- | --- |
| Space | Play/pause selected deck |
| Z / X | Play/pause deck A / B |
| C | Jump to cue 1, or set it if empty |
| G | Quickly set the next empty cue pad |
| 1–8 | Selected deck hot cues 1–8 |
| Q W E R / U I O P | Deck A / B hot cues 1–4 |
| Shift + cue key (including C) | Set/replace that cue at the current playhead |
| S | Sync selected deck |
| L | Toggle selected deck loop |
| [ / ] | Halve/double loop size, from 1 to 16 bars |
| Left / Right | Jump backward/forward 1 bar |
| Shift + Left / Right | Jump backward/forward 4 bars |
| F | Center crossfader |
| Escape | Close the open panel |

Empty cue pads set a point on first use; existing cues jump without changing
play/pause. Mouse clicks use the same behavior, including Shift to replace.
Cue points are saved with the track. G leaves existing cues intact when all
8 slots are full. Shortcuts do not run in import/help dialogs or file pickers,
and holding a key does not repeatedly trigger an action.

The library fills the remaining window height. Use **+ Files** or **+ Folder**
for local imports (folders include subfolders; duplicate paths are skipped),
or **Spotify** for playlist import and progress details. Each local folder is
its own playlist under **LOCAL FOLDERS**: tracks belong to their immediate
parent folder, so nested folders and equally named folders at different paths
stay separate. Reimports add files without duplicating or clearing existing
members. Existing local tracks are grouped on the next launch. Startup and
completed imports select a playlist; **All Tracks** is an explicit combined view.
Native file
selection is asynchronous and folder scanning runs on a worker. Drag a library
track onto either deck to load it; the receiving deck highlights during the drag.
Loading a track manually takes over from Automix.

The waveform uses a symmetric peak envelope colored by low/mid/high spectral
energy, with a darker RMS body and a fixed playhead. Beat and downbeat markers
come from stored analysis; tracks without a measured grid show no invented bar
lines. Tempo changes preserve the source-time grid. This is an independent
renderer inspired by DJ workstations, not a pixel-identical reproduction.

Drag a horizontal or vertical waveform to move the audio beneath its fixed
playhead. Release preserves play/pause and commits the final pointer position.
Press **SYNC / S** on the following deck to match the other deck's tempo and
beat phase; press again to disengage at the current tempo. **MASTER** identifies
the reference deck, **SYNC …** means armed/acquiring, and a filled green **SYNC**
means the grids are locked. **GRID … / NO GRID** indicates analysis in progress
or no usable grid. Old analysis caches are refreshed on a background worker.
Analysis payloads and file verification hashes persist in `library.db` across
restarts. Library refresh queues background preparation and updates each row
when it completes. Verification is reused only while the filesystem revision
(size, nanosecond modification/change times, device and inode) matches; changed
content or analysis versions trigger fresh analysis. Deck loading still checks
the decoded file against the full content hash.
SYNC can be pressed during analysis: it waits for both grids and engages
automatically. Press again to cancel; loading another track or taking manual
control cancels the pending request as well.
Master tempo changes are followed continuously, including half/double tempo
matching. Scrubbing, reverse, or changing the follower's tempo takes manual
control; enabling Automix gives its plan control of timing.
Research sources, behavior and limitations are in [BEAT_SYNC.md](BEAT_SYNC.md).


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

The planner uses local tempo maps, Camelot compatibility, measured phrase
boundaries, energy/foreground/kick scoring, 8/16/32-bar overlap search, 1:1 and
2:1 bar maps, and inherited performance offsets. On macOS 12+, offline analysis
also attempts a bounded, on-device Apple Sound Analysis vocal classifier; no
model download or Python runtime is needed. Model failures retain the DSP result.
The original nine catalog envelopes remain available with `smooth: false`.
Performance measurements and the model roadmap are in
[ANALYSIS_ENHANCEMENT.md](ANALYSIS_ENHANCEMENT.md).

Implementation details, verification and current limitations are in
[AUTOMIX.md](AUTOMIX.md).

Audio DSP behavior, regression checks and remaining limitations are documented
in [AUDIO_DSP.md](AUDIO_DSP.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for checks and Conventional Commits.
See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for bundled code licenses.

### Audio effects

Each deck has four insert slots with 70 selectable audio FX. Use **⋯** on a slot
for the grouped catalogue, beat timing and parameter editor. See
[FX_CATALOGUE.md](FX_CATALOGUE.md) for the Rekordbox/djay category comparison and
[AUDIO_DSP.md](AUDIO_DSP.md) for algorithms and validation limits.
