<div align="center">

<img src="assets/branding/app-icon-1024.png" alt="Mixless" width="112">

# Mixless

**Your tracks. One continuous flow.**

A native Mac DJ workspace with local AI analysis, dual decks, and musical AutoMix.

[Website](https://mixless.alkinum.com) · [User guide](https://mixless.alkinum.com/docs) · [中文指南](https://mixless.alkinum.com/docs/zh) · [Downloads](https://github.com/backrunner/mixless/releases)

[![CI](https://github.com/backrunner/mixless/actions/workflows/ci.yml/badge.svg)](https://github.com/backrunner/mixless/actions/workflows/ci.yml)
[![macOS](https://img.shields.io/badge/macOS-12%2B-18181b)](https://github.com/backrunner/mixless/releases)
[![MPL 2.0](https://img.shields.io/badge/license-MPL_2.0-f5a65b)](LICENSE)

</div>

![Mixless: dual decks, stem controls, spectral waveforms, mixer, effects and a local music library](assets/screenshots/workspace.png)

Mixless turns a folder of music into a continuous set. Mix by hand, let AutoMix plan the handoffs, or take control whenever you want. Built in Rust with a GPU-rendered [GPUI](https://github.com/zed-industries/gpui) interface and a native audio engine.

- **Musical AutoMix.** Plans around phrases, tempo, key, energy and vocals, with blends, bass swaps and structural cuts. Your IN/OUT cues set the boundaries.
- **Hands-on decks.** Beat sync, key lock, hot cues, loops, scratching, four FX slots per deck and 70 effects.
- **Local AI preparation.** On-device stem separation and note analysis. Shape vocals, drums and instruments independently; no cloud inference or Python runtime.
- **Your library.** Import files and folders, organize playlists, reorder tracks by dragging, and bring in Spotify playlist metadata.
- **Your setup.** Separate master and headphone outputs, persistent preferences, and MIDI learning and mapping for decks, mixer and library controls.

Audio and analysis stay on your Mac. First use downloads verified models. Spotify supplies metadata, not mixing audio; optional audio acquisition requires separately installed tools. See [Privacy](PRIVACY.md).

## Get started

Download a macOS installer from [Releases](https://github.com/backrunner/mixless/releases), open the DMG, and move Mixless to Applications. The app targets **macOS 12+**, with Apple Silicon and Intel release builds. Beta versions are marked **Pre-release**.

1. Add music with **+ Files** or **+ Folder**.
2. Check your output in **Mixless → Preferences…** (**Cmd+,**).
3. Select a playlist and press **AUTO**, or drag a track onto a deck and press Play.

See the [first-mix guide](apps/site/content/docs/index.md), [AutoMix guide](apps/site/content/docs/automix.md), [MIDI guide](apps/site/content/docs/midi.md), and [keyboard shortcuts](apps/site/content/docs/shortcuts.md). The [official site](https://mixless.alkinum.com/docs) contains the full English and Chinese handbook.

## Develop

Requires current stable Rust and full Xcode with the Metal compiler.

```bash
./dev.sh --check       # Check prerequisites
./dev.sh               # Build and launch
./dev.sh --build       # Build without launching

cargo test --locked --workspace --exclude mixless-desktop
cargo test --locked -p mixless-desktop
```

The first build may take longer. The script selects an Xcode installation with Metal support, respects `DEVELOPER_DIR`, and uses your normal local app library. Rerun it after code changes. Version, revision and build time appear in **Mixless → About Mixless**.

| Path | Contents |
| --- | --- |
| `apps/desktop` | Native GPUI application |
| `apps/site` | Official website and bilingual guide, built with svedocs |
| `crates/mixless-engine`, `mixless-protocol` | Real-time audio, commands and snapshots |
| `crates/mixless-library` | Library and playlists |
| `crates/mixless-analyze`, `mixless-stems`, `mixless-mixplan` | Analysis, local inference and transition planning |
| `crates/mixless-midi` | Controller mapping |
| `crates/mixless-spotify`, `mixless-acquire*` | Playlist metadata and optional audio acquisition |
| `crates/mixless-tools` | Packaging and release helpers |

## Website

The site uses **svedocs 0.2.1**, with a custom landing page, theme, local search, and complete English/Chinese content. Requires Node.js 22.12+ and pnpm.

```bash
pnpm -C apps/site install --frozen-lockfile
pnpm -C apps/site dev

pnpm -C apps/site check
pnpm -C apps/site check:content
pnpm -C apps/site build
```

See [apps/site/README.md](apps/site/README.md) for content, asset and hosting instructions.

## Contribute & license

Read [CONTRIBUTING.md](CONTRIBUTING.md) for contribution checks and [RELEASING.md](RELEASING.md) for signed macOS releases. Engineering references remain in [`.agents/`](.agents/README.md).

Mixless is licensed under [MPL 2.0](LICENSE). See [NOTICE](NOTICE), [third-party notices](THIRD_PARTY_NOTICES.md), [privacy](PRIVACY.md), and [security reporting](SECURITY.md).
