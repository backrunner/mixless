#!/bin/bash
# Build the Mixless installer DMG from a signed, notarized .app bundle.
# Usage: scripts/make-macos-dmg.sh <Mixless.app> <output.dmg> [volume name]
# Requires dmgbuild on PATH: python3 -m pip install dmgbuild (or pipx).
set -euo pipefail

app="${1:?usage: make-macos-dmg.sh <Mixless.app> <output.dmg> [volume name]}"
out="${2:?usage: make-macos-dmg.sh <Mixless.app> <output.dmg> [volume name]}"
volname="${3:-Mixless}"
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

[ -d "${app}/Contents/MacOS" ] || { echo "Not an app bundle: ${app}" >&2; exit 1; }
app="$(cd -- "$(dirname -- "${app}")" && pwd)/$(basename -- "${app}")"
command -v dmgbuild >/dev/null 2>&1 || {
    echo "dmgbuild is required (python3 -m pip install dmgbuild)" >&2
    exit 1
}

tmp="$(mktemp -d "${TMPDIR:-/tmp}/mixless-dmg.XXXXXX")"
trap 'rm -rf "${tmp}"' EXIT
swift "${root}/scripts/generate-dmg-background.swift" "${tmp}"

mkdir -p "$(dirname "${out}")"
rm -f "${out}"
dmgbuild -s "${root}/scripts/dmg-settings.py" \
    -D "app=${app}" \
    -D "background=${tmp}/background.png" \
    "${volname}" "${out}"

# dmgbuild positions icons by scripting Finder on a mounted read-write
# scratch image, and Finder stamps com.apple.FinderInfo on the .app —
# `codesign --verify --strict` then rejects the installed bundle as
# detritus. Roundtrip the image and strip every xattr off the app.
rw="${tmp}/rw.dmg"
mnt="${tmp}/mnt"
mkdir -p "${mnt}"
hdiutil convert "${out}" -format UDRW -o "${rw}" >/dev/null
hdiutil attach "${rw}" -nobrowse -mountpoint "${mnt}" >/dev/null
xattr -cr "${mnt}/Mixless.app"
hdiutil detach "${mnt}" -quiet || hdiutil detach "${mnt}" -force
# hdiutil appends .dmg to -o targets lacking the suffix — keep it.
hdiutil convert "${rw}" -format UDZO -o "${tmp}/final.dmg" >/dev/null
mv "${tmp}/final.dmg" "${out}"

echo "DMG: ${out}"
