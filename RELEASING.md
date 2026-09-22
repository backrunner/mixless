# Releasing mixless

The release pipeline (`.github/workflows/release.yml`) builds a universal
(Apple Silicon + Intel) `Mixless.app`, signs and notarizes it with your Apple
Developer ID, and publishes two installers on the GitHub release for the tag:

- `Mixless-macOS-<version>.dmg`: standard; downloads models on first use.
- `Mixless-macOS-<version>-with-models.dmg`: includes the same pinned HTDemucs
  and Basic Pitch models for first-use offline analysis on Apple Silicon.

Both variants use the same universal executable, bundle ID and user library.
Intel Macs still do not support stem/note inference. Bundled weights live in
`Contents/Resources/models`, are verified before packaging and loading, and are
read directly without copying or writing into the app. Storage cleanup only
removes downloaded copies. Both variants retain the upstream license notices.

## Channels

| Tag you push        | Channel | GitHub release                        |
| ------------------- | ------- | ------------------------------------- |
| `v1.2.3`            | stable  | normal release, marked **Latest**     |
| `v1.2.3-beta.1`     | beta    | **prerelease**, never marked Latest   |

The channel is baked into the build (`MIXLESS_CHANNEL`, the
`MixlessReleaseChannel` Info.plist key, and the About window). Each release
also ships `mixless-<channel>-latest.json`, a small manifest with the version,
DMG URLs and SHA-256 values that the in-app updater polls. The platform keys
are `darwin-universal` (standard, retained for older clients) and
`darwin-universal-bundled` (with models); `SHA256SUMS.txt` covers both DMGs.
Stable builds read the
asset on `releases/latest`; beta builds list releases through the GitHub API
and select the highest semantic beta version carrying the beta manifest across
all release pages. Release creation order does not determine version precedence.

## Updates

`apps/desktop/src/update.rs` drives self-updates on every launch (release
channels only; dev builds skip) and from **mixless → Check for Updates…**:

- The manifest for the build's channel is fetched and compared with
  `CARGO_PKG_VERSION` by semver — only a strictly newer version installs, so
  an older feed can never downgrade the app.
- The DMG is downloaded to a temp dir, its SHA-256 must match the manifest,
  and the `.app` inside must pass `codesign --deep --strict`, `spctl`
  notarization assessment, a bundle `Identifier`/`TeamIdentifier` match with
  the running app, and a `CFBundleShortVersionString` match with the
  manifest.
- The updater selects the installed model variant and verifies that the new
  bundle has the same variant. If a feed lacks the bundled installer it fails
  rather than silently replacing an offline installation with the standard app.
- The complete bundle is copied into a private sibling directory and verified
  again before activation. macOS atomically exchanges the staged and installed
  directories; copy or verification failure leaves the installed app untouched.
  The app never restarts itself — a banner asks the user to restart to finish.
- Manual checks remain visible even when they join a background check. Once an
  update is installed, further checks preserve the restart notice instead of
  downloading again. Both the running version and the installed bundle version
  are checked to avoid replacing an already newer on-disk app. A per-app
  installation lock serializes processes, and the installed version is checked
  again under that lock after downloading and staging.

`MIXLESS_UPDATE_TARGET=<path>` overrides the install destination for tests.

Unit and local HTTP regression tests run in desktop CI. A separate native
integration check downloads a published, signed DMG and installs it into a
temporary app directory, exercising codesign, Gatekeeper, stapler and a headless
executable restart. It requires a local copy of that release's manifest:

```sh
MIXLESS_TEST_UPDATE_MANIFEST=/absolute/path/mixless-beta-latest.json \
  cargo test --locked -p mixless-desktop published_package_roundtrip \
  -- --ignored --nocapture --test-threads=1
```

This check does not replace `/Applications/Mixless.app` or open a user library.
Version-upgrade decisions are covered separately by the SemVer/channel tests;
the native check reinstalls the published package to exercise replacement.

## Data compatibility

Beta runs ahead of stable, so a beta → stable downgrade must never corrupt
or strand the user's library:

- `library.db` carries `PRAGMA user_version` = `SCHEMA_VERSION`
  (`crates/mixless-library/src/schema.rs`). Bump it on every schema change.
- Schema changes must be **additive** — new tables, or new columns that are
  nullable or have defaults. Statements always name their columns, so an
  older build tolerates extra columns. If a change cannot be additive, older
  builds detect it on open (`LibraryError::NewerSchema`), move the database
  and its WAL/SHM journals into a unique `library.db.unsupported-*` directory,
  and only then start a fresh library. A failed move rolls back the files
  already moved; any files that cannot be restored remain in the backup.
- Derived data is gated per record: `track_analysis`/`track_waveforms`/
  `cue_versions` carry payload versions, so a newer build's analysis is
  invisible to an older build and simply re-analyzes. Never read a payload
  without checking its version.
- When adding a table or naming a new column in a query, extend
  `REQUIRED_COLUMNS` in `schema.rs` so the downgrade check stays accurate.

## Required secrets

Set these under **Settings → Secrets and variables → Actions**:

Signing (required):

- `APPLE_CERTIFICATE` — base64 of the exported `.p12` for your
  **Developer ID Application** certificate
  (`base64 -i cert.p12 | pbcopy`). `CSC_LINK` also works.
- `APPLE_CERTIFICATE_PASSWORD` — the export password (`CSC_KEY_PASSWORD` works).
- `APPLE_SIGNING_IDENTITY` — e.g. `Developer ID Application: Name (TEAMID)`.
- `APPLE_TEAM_ID` — 10-character team id.

Notarization — one of:

- `APPLE_ID` + `APPLE_PASSWORD` (app-specific password from
  appleid.apple.com), or
- `APPLE_API_KEY_ID` + `APPLE_API_ISSUER` + `APPLE_API_KEY_BASE64`
  (base64 of the `.p8` App Store Connect key).

## Cutting a release

1. Validate the workspace, desktop and website checks, and commit the intended release state. Add user-facing changes and known limitations to `.github/release-notes/<tag>.md`; the workflow attaches this file to the release.
2. Push a tag: `git tag v1.2.3 && git push origin v1.2.3`
   (or `v1.2.3-beta.1` for beta).
3. The workflow stamps the workspace version, builds both architectures once,
   downloads checksum-pinned models, and assembles both app variants. Each app
   and DMG is independently signed, notarized and stapled. It verifies both
   mount layouts and cold-loads both bundled inference sessions directly from
   the read-only DMG with model downloads disabled before publishing.
4. Re-running for an existing tag: **Actions → Release → Run workflow** with
   the tag (and optionally an explicit channel override). Published releases
   are never overwritten — the workflow fails instead.

## Install behavior

The DMG opens to a single `Mixless.app` plus an `Applications` alias.
Double-clicking the app inside the DMG copies it to `/Applications` (falling
back to `~/Applications` without admin rights), removes the quarantine flag,
and relaunches the installed copy. Dragging to `Applications` also works.

Two env hooks exist for tests and CI:

- `MIXLESS_INSTALL_ONLY=1` — perform the install check, then exit before
  relaunching or starting the UI.
- `MIXLESS_INSTALL_DIR=<dir>` — install into `<dir>` instead of
  `/Applications`.

## Local packaging

```sh
cargo build --release -p mixless-desktop
cargo run -p mixless-tools -- bundle target/release/mixless dist/Mixless.app \
  --version 1.2.3 --build-number 1 --channel stable
python3 -m venv target/dmg-venv && target/dmg-venv/bin/pip install dmgbuild
PATH="$PWD/target/dmg-venv/bin:$PATH" \
  scripts/make-macos-dmg.sh dist/Mixless.app dist/Mixless-macOS-1.2.3.dmg
scripts/verify-macos-dmg.sh dist/Mixless-macOS-1.2.3.dmg
```

Signing/notarization locally uses the same `codesign`/`notarytool` commands as
the workflow. Local layout checks need a signed app (ad-hoc signing with
`codesign --force --sign -` is sufficient for local checks, not distribution).

To assemble the variant with models from that same executable:

```sh
cargo run --locked -p mixless-tools -- download-models target/release-models
cargo run --locked -p mixless-tools -- bundle target/release/mixless \
  dist/bundled/Mixless.app --models target/release-models
cargo run --locked -p mixless-tools -- verify-models dist/bundled/Mixless.app/Contents/Resources/models
dist/bundled/Mixless.app/Contents/MacOS/mixless --check-bundled-models
```

`download-models` reuses only checksum-valid files; packaging refuses missing
or damaged weights. Omitting `--models` removes stale weights if an output
directory is reused. Sign/notarize this bundle, then create its DMG and run
`scripts/verify-macos-dmg.sh <with-models.dmg> bundled` on Apple Silicon.
The diagnostic command loads actual inference sessions without AppKit, audio
devices, user data or network access. For a full new-audio inference check:

```sh
MIXLESS_TEST_BUNDLED_MODELS="$PWD/target/release-models" \
  cargo test --locked -p mixless-stems \
  bundled_models_analyze_new_audio_offline_after_download_cache_cleanup \
  -- --ignored --nocapture
```

The website keeps `/download` as standard and offers
`/download?variant=bundled` for models, with `&channel=beta` when requested.
It never substitutes a standard download for a requested bundled installer.

Note: Homebrew's `rust` links `rust-objcopy` to the `llvm` formula. If release
builds fail with `unable to run 'rust-objcopy'`, run `brew install llvm`, or
build with `CARGO_PROFILE_RELEASE_STRIP=none`. CI is unaffected: rustup's
`rustc` component bundles a real `rust-objcopy`.
