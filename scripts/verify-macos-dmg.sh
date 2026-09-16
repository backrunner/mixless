#!/bin/bash
# Mount a built DMG read-only and check the install layout.
# Usage: scripts/verify-macos-dmg.sh <file.dmg>
set -euo pipefail

dmg="${1:?usage: verify-macos-dmg.sh <file.dmg>}"
mount="$(mktemp -d "${TMPDIR:-/tmp}/mixless-dmg-mount.XXXXXX")"
cleanup() { hdiutil detach "${mount}" -quiet 2>/dev/null || true; rm -rf "${mount}"; }
trap cleanup EXIT

hdiutil attach "${dmg}" -readonly -nobrowse -noautoopen -mountpoint "${mount}" >/dev/null

fail() { echo "verify-macos-dmg: $*" >&2; exit 1; }
[ -x "${mount}/Mixless.app/Contents/MacOS/mixless" ] || fail "Mixless.app missing or not executable"
[ -f "${mount}/Mixless.app/Contents/Resources/Mixless.icns" ] || fail "app icon missing"
[ -L "${mount}/Applications" ] || fail "Applications symlink missing"
[ "$(readlink "${mount}/Applications")" = "/Applications" ] || fail "Applications symlink target wrong"
[ -f "${mount}/.DS_Store" ] || fail "Finder layout (.DS_Store) missing"
plutil -lint "${mount}/Mixless.app/Contents/Info.plist" >/dev/null || fail "Info.plist invalid"
# The updater runs this exact check before installing; keep the pipeline
# honest so a detritus xattr can never ship again.
codesign --verify --deep --strict "${mount}/Mixless.app" || fail "app fails strict codesign"
echo "verify-macos-dmg: OK"
