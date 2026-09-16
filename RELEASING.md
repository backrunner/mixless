# Releasing Mixless

The release pipeline (`.github/workflows/release.yml`) builds a universal
(Apple Silicon + Intel) `Mixless.app`, signs and notarizes it with your Apple
Developer ID, and publishes a `Mixless-macOS-<version>.dmg` on the GitHub
release for the tag.

## Channels

| Tag you push        | Channel | GitHub release                        |
| ------------------- | ------- | ------------------------------------- |
| `v1.2.3`            | stable  | normal release, marked **Latest**     |
| `v1.2.3-beta.1`     | beta    | **prerelease**, never marked Latest   |

The channel is baked into the build (`MIXLESS_CHANNEL`, the
`MixlessReleaseChannel` Info.plist key, and the About window). Each release
also ships `mixless-<channel>-latest.json`, a small manifest with the version,
DMG URL and SHA-256 that a future updater can poll. Stable is
`releases/latest` on the GitHub API; beta builds are prereleases.

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

1. Push a tag: `git tag v1.2.3 && git push origin v1.2.3`
   (or `v1.2.3-beta.1` for beta).
2. The workflow stamps the workspace version, builds both architectures,
   bundles, signs, notarizes and staples the app, then builds, signs,
   notarizes and staples the DMG, verifies the mount layout, and publishes a
   draft-turned-public release.
3. Re-running for an existing tag: **Actions → Release → Run workflow** with
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
the workflow; unsigned DMGs still build and verify for layout checks.

Note: Homebrew's `rust` links `rust-objcopy` to the `llvm` formula. If release
builds fail with `unable to run 'rust-objcopy'`, run `brew install llvm`, or
build with `CARGO_PROFILE_RELEASE_STRIP=none`. CI is unaffected: rustup's
`rustc` component bundles a real `rust-objcopy`.
