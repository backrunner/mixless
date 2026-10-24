#!/bin/bash
# Mount a built DMG read-only and check the install layout.
# Usage: scripts/verify-macos-dmg.sh <file.dmg> [standard|bundled]
set -euo pipefail

dmg="${1:?usage: verify-macos-dmg.sh <file.dmg>}"
variant="${2:-standard}"
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
python3 - "${mount}/Mixless.app" <<'PY'
from pathlib import Path
import subprocess
import sys
import tarfile

app = Path(sys.argv[1])
binary = app / "Contents/MacOS/mixless"
architectures = subprocess.check_output(["lipo", "-archs", str(binary)], text=True).split()
assert set(architectures) == {"arm64", "x86_64"}, architectures
for architecture in architectures:
    links = subprocess.check_output(["otool", "-arch", architecture, "-L", str(binary)], text=True)
    for line in links.splitlines()[1:]:
        assert not any(part in line for part in (
            "/opt/homebrew/", "/usr/local/", "libavcodec", "libavformat",
            "libavutil", "libavfilter", "libswresample", "libx265",
        )), f"External audio dependency in {architecture}: {line}"
resources = app / "Contents/Resources"
for name in ("FFmpeg-COPYING.LGPLv2.1.txt", "FFmpeg-LICENSE.md.txt", "FFmpeg-BUILD.md"):
    assert (resources / "Licenses" / name).stat().st_size > 0, name
with tarfile.open(resources / "Sources/ffmpeg-9.0.2-source.tar.gz") as archive:
    for name in ("COPYING.LGPLv2.1", "configure", "libavcodec/aacenc.c"):
        assert archive.getmember(f"ffmpeg-9.0.2/{name}").size > 0, name
print("verify-macos-dmg: universal built-in audio and corresponding sources OK")
PY
models="${mount}/Mixless.app/Contents/Resources/models"
case "${variant}" in
  standard) [ ! -e "${models}" ] || fail "standard installer contains models" ;;
  bundled)
    cargo run --locked -p mixless-tools -- verify-models "${models}"
    [ "$(uname -m)" = "arm64" ] || fail "bundled model inference check requires Apple Silicon"
    MIXLESS_MODELS_OFFLINE=1 "${mount}/Mixless.app/Contents/MacOS/mixless" --check-bundled-models
    ;;
  *) fail "unknown variant: ${variant}" ;;
esac
echo "verify-macos-dmg: OK"
