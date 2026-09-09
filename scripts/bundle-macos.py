#!/usr/bin/env python3
"""Assemble a local unsigned .app with Mixless's icon (after ./dev.sh --build)."""
import argparse
import plistlib
import shutil
import subprocess
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=root / 'target/debug/mixless')
parser.add_argument('--output', type=Path, default=root / 'target/app/Mixless.app')
args = parser.parse_args()
if not args.binary.is_file():
    parser.error('Build the desktop app first, or supply --binary.')
if args.output.suffix != '.app':
    parser.error('--output must end in .app')
version = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['version']
contents = args.output / 'Contents'
macos = contents / 'MacOS'
resources = contents / 'Resources'
macos.mkdir(parents=True, exist_ok=True)
resources.mkdir(parents=True, exist_ok=True)
shutil.copy2(args.binary, macos / 'mixless')
shutil.copy2(root / 'apps/desktop/resources/Mixless.icns', resources / 'Mixless.icns')
info = {
    'CFBundleName': 'Mixless',
    'CFBundleDisplayName': 'Mixless',
    'CFBundleIdentifier': 'app.mixless.desktop',
    'CFBundleExecutable': 'mixless',
    'CFBundleIconFile': 'Mixless.icns',
    'CFBundlePackageType': 'APPL',
    'CFBundleShortVersionString': version,
    'CFBundleVersion': '1',
    'LSMinimumSystemVersion': '12.0',
    'NSHighResolutionCapable': True,
    'NSSupportsAutomaticGraphicsSwitching': True,
    'NSHumanReadableCopyright': 'Mixless contributors. MPL-2.0.',
}
with (contents / 'Info.plist').open('wb') as file:
    plistlib.dump(info, file)
subprocess.run(['plutil', '-lint', str(contents / 'Info.plist')], check=True)
print(f'Local unsigned app: {args.output}')
