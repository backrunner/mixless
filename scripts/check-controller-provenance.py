#!/usr/bin/env python3
"""Check specific controller source/artifact regressions, not legal clearance.

Independently authored by mixless contributors, MPL-2.0.
Scans Git's current tracked and non-ignored untracked files, without vendor access.
"""
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
CONTROLLERS = "crates/mixless-controllers/"
AUDIT_KEYS = {
    "format", "version", "factory_version", "tsi", "csi", "audio", "hardware_verified",
    "sha256", "devices", "device_class", "addressing", "input_count", "output_count",
    "evidence", "wire_protocol_confirmed", "file", "qml_files", "device_directory",
    "device_name", "master", "cue", "mac_driver_name",
}
NATIVE_MAGIC = {b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf",
                b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca", b"\x7fELF"}


def audit_fields(value, errors):
    if isinstance(value, dict):
        for key, nested in value.items():
            if key not in AUDIT_KEYS:
                errors.append(f"Unexpected source-audit field: {key}")
            if key in {"wire_protocol_confirmed", "hardware_verified"} and nested is not False:
                errors.append(f"Source inspection cannot certify {key}")
            audit_fields(nested, errors)
    elif isinstance(value, list):
        for nested in value:
            audit_fields(nested, errors)


def check(root, paths):
    errors = []
    for relative in paths:
        path = root / relative
        if relative.startswith(CONTROLLERS) and path.is_symlink():
            errors.append(f"Symlink in controller source delivery: {relative}")
            continue
        if not path.is_file():
            continue
        lower = path.name.lower()
        if path.suffix.lower() in {".tsi", ".qml"} or re.search(r"\.nc(?:k|m)[a-z0-9]*$", lower):
            errors.append(f"Vendor controller source/container format in delivery: {relative}")
        if relative.startswith(CONTROLLERS):
            with path.open("rb") as source:
                magic = source.read(4)
            if magic in NATIVE_MAGIC or magic.startswith(b"MZ"):
                errors.append(f"Native executable in controller source tree: {relative}")
            if path.suffix == ".rs":
                text = path.read_text()
                if re.search(r"libloading|\bdlopen\b|/Applications/Native Instruments|/Library/Application Support/Native Instruments", text):
                    errors.append(f"Vendor-loading/source-access primitive in controller Rust code: {relative}")
                if re.search(r'#\[link\s*\(\s*name\s*=\s*"(?:NI|NHL|Traktor)', text, re.I):
                    errors.append(f"Vendor native link in controller code: {relative}")
        if relative == CONTROLLERS + "NI_PROVENANCE.json":
            record = json.loads(path.read_text())
            if record.get("implementation_status") != "independent_implementation" or record.get("strict_clean_room_verified") is not False:
                errors.append("This audit does not certify a strict clean-room process")
            if record.get("exposure", {}).get("research_and_implementation_separated") is not False:
                errors.append("Recorded implementation exposure must not be erased")
            if record.get("legal_clearance") != "not_assessed":
                errors.append("This source/artifact review cannot grant legal clearance")
        if relative == CONTROLLERS + "research/ni/traktor-4.4.1-audit.json":
            audit_fields(json.loads(path.read_text()), errors)
    required = {CONTROLLERS + "NI_PROVENANCE.json", CONTROLLERS + "CLEAN_ROOM_REVIEW.md"}
    for relative in sorted(required):
        if relative not in paths or not (root / relative).is_file():
            errors.append(f"Missing provenance record: {relative}")
    return errors


def main():
    paths = subprocess.check_output(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=ROOT)
    files = sorted(set(paths.decode().rstrip("\0").split("\0")))
    try:
        errors = check(ROOT, files)
    except (OSError, ValueError) as error:
        errors = [str(error)]
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print("Controller artifact/source checks passed. Strict clean room and legal clearance are not certified.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
