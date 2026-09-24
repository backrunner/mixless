---
title: "Build from source"
description: "Run the native app or work on the official website."
order: 9
image: /images/social.png
imageAlt: "mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

## Desktop application

Install current stable Rust and full Xcode with the Metal compiler. The standalone Command Line Tools are not sufficient.

```bash
git clone https://github.com/backrunner/mixless.git
cd mixless
./dev.sh --check
./dev.sh
```

The script finds an Xcode installation with Metal support; an explicit `DEVELOPER_DIR` takes precedence. Use `./dev.sh --build` to build without launching. Development uses the normal local application library and does not watch files automatically.

The first build optimizes the audio engine and UI dependencies and can take longer. Rerun after changes. **mixless → About mixless** shows the version, Git revision, and build time.

## Workspace

| Area | Responsibility |
| --- | --- |
| `apps/desktop` | Native GPUI interface and application state |
| `mixless-engine` / `mixless-protocol` | Audio graph, transport, commands and snapshots |
| `mixless-library` | Local library, playlists and persisted data |
| `mixless-analyze` / `mixless-stems` | Offline music analysis and local inference |
| `mixless-mixplan` | Musical transition planning |
| `mixless-midi` | Controller mapping and input |
| `mixless-spotify` / `mixless-acquire*` | Playlist metadata and optional local audio acquisition |
| `mixless-tools` | Packaging and release helpers |
| `apps/site` | This svedocs website |

The desktop UI does not implement DSP. Real-time processing stays in the audio engine.

### Deck controls and MIDI mapping

Any change to the deck-area layout or its knobs, faders, buttons, or control
order must update the MIDI target catalogue and the Preferences MIDI wireframe
in the same change. Keep the native controls and mapping targets aligned in
count, order, labels, and semantics; update the mapping tests and user docs
when a control changes.

## Website

From the repository root, with Node.js 22.12+ and pnpm:

```bash
pnpm -C apps/site install --frozen-lockfile
pnpm -C apps/site dev
```

The website uses svedocs and svedocs-cli 0.2.1 with a custom landing page, navigation, and theme. English content lives directly under `content/docs` and `content/pages`; Chinese mirrors it under each root's `zh` directory.

```bash
pnpm -C apps/site check
pnpm -C apps/site check:content
pnpm -C apps/site build
```

The build emits a static site in `apps/site/build`. Set `SITE_URL` when building for a different domain. Run `pnpm -C apps/site assets` after changing the source screenshots or branding.

## Contribute

Read [CONTRIBUTING.md](https://github.com/backrunner/mixless/blob/main/CONTRIBUTING.md) for checks and commit conventions, and [RELEASING.md](https://github.com/backrunner/mixless/blob/main/RELEASING.md) for signed macOS releases.

mixless is licensed under [MPL 2.0](https://github.com/backrunner/mixless/blob/main/LICENSE). Attribution and component licenses are listed in [THIRD_PARTY_NOTICES.md](https://github.com/backrunner/mixless/blob/main/THIRD_PARTY_NOTICES.md).
