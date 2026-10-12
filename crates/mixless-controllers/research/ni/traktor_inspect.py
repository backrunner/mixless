#!/usr/bin/env python3
"""Read-only Traktor NI source audit. No preset conversion or device access.

Independently authored by mixless contributors, MPL-2.0.
Only device/control descriptors and output channel facts are inspected. Traktor
command assignments, modifiers and mapping logic are not decoded or exported.
"""

import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import struct
import sys
import xml.etree.ElementTree as ET

MAX_TSI = 16 * 1024 * 1024
MAX_BINARY = 512 * 1024 * 1024
MAX_CONTROLS = 65536
NI_CSI = {"MX2", "X1MK3", "Z1MK2", "S2MK3", "S3", "S4MK3", "D2", "S5", "S8"}


def read_bounded(path, limit):
    with Path(path).open("rb") as source:
        data = source.read(limit + 1)
    if len(data) > limit:
        raise ValueError("Source exceeds inspection size limit")
    return data


def parse_xml(data):
    # Restrict this inspector to the UTF-8 vendor files observed locally.
    text = data.decode("utf-8-sig")
    if "<!DOCTYPE" in text or "<!ENTITY" in text:
        raise ValueError("DTD and entity declarations are not supported")
    try:
        return ET.fromstring(text)
    except ET.ParseError as error:
        raise ValueError(f"Invalid source XML: {error}") from error


def u32(data, offset, end):
    if offset + 4 > end:
        raise ValueError("Truncated integer")
    return struct.unpack_from(">I", data, offset)[0]


def chunks(data, start, end):
    """Observed DIOM structure: four-byte tag and big-endian payload length."""
    while start < end:
        if start + 8 > end:
            raise ValueError("Truncated chunk header")
        tag = data[start:start + 4]
        if not all(65 <= byte <= 90 for byte in tag):
            raise ValueError("Invalid chunk tag")
        size = u32(data, start + 4, end)
        stop = start + 8 + size
        if stop > end:
            raise ValueError("Chunk extends past its parent")
        yield tag, start + 8, stop
        start = stop


def text_field(data, start, end):
    count = u32(data, start, end)
    stop = start + 4 + count * 2
    if count > 4096 or stop > end:
        raise ValueError("Invalid UTF-16 field length")
    return data[start + 4:stop].decode("utf-16-be"), stop


def one(items, tag):
    found = [item for item in items if item[0] == tag]
    if len(found) != 1:
        raise ValueError(f"Expected one {tag.decode()} chunk")
    return found[0][1:]


def descriptors(data, start, end):
    count = u32(data, start, end)
    if count > MAX_CONTROLS:
        raise ValueError("Too many control descriptors")
    result = []
    for tag, first, last in chunks(data, start + 4, end):
        if tag != b"DCDT":
            raise ValueError("Unsupported descriptor chunk")
        name, _ = text_field(data, first, last)
        result.append(name)
    if len(result) != count:
        raise ValueError("Control descriptor count mismatch")
    return result


def inspect_tsi(data, include_controls=False):
    if len(data) > MAX_TSI:
        raise ValueError("TSI exceeds inspection size limit")
    root = parse_xml(data)
    if root.tag != "NIXML":
        raise ValueError("Expected NIXML source")
    entries = [e for e in root.iter("Entry")
               if e.get("Name") == "DeviceIO.Config.Controller"]
    if not entries:
        return {"sha256": hashlib.sha256(data).hexdigest(), "devices": [],
                "evidence": "source_only", "wire_protocol_confirmed": False}
    if len(entries) != 1:
        raise ValueError("Expected one controller configuration entry")
    encoded = entries[0].get("Value", entries[0].text or "")
    payload = base64.b64decode("".join(encoded.split()), validate=True)
    top = list(chunks(payload, 0, len(payload)))
    start, end = one(top, b"DIOM")
    if len(top) != 1:
        raise ValueError("Trailing data after DIOM")
    start, end = one(list(chunks(payload, start, end)), b"DEVS")
    count = u32(payload, start, end)
    if count > 256:
        raise ValueError("Too many device definitions")
    devices = []
    for tag, first, last in chunks(payload, start + 4, end):
        if tag != b"DEVI":
            raise ValueError("Unsupported device definition")
        device_class, pos = text_field(payload, first, last)
        first, last = one(list(chunks(payload, pos, last)), b"DDAT")
        # Skip DDPT (ports/serials) and DDCB (command assignments) entirely.
        first, last = one(list(chunks(payload, first, last)), b"DDDC")
        tables = list(chunks(payload, first, last))
        controls = {}
        for direction, tag in [("inputs", b"DDCI"), ("outputs", b"DDCO")]:
            first, last = one(tables, tag)
            controls[direction] = descriptors(payload, first, last)
        device = {
            "device_class": device_class,
            "addressing": "native_symbols" if device_class.startswith("Traktor.")
                          else "generic_midi" if device_class == "Generic MIDI"
                          else "unresolved",
            "input_count": len(controls["inputs"]),
            "output_count": len(controls["outputs"]),
        }
        if include_controls:
            device["controls"] = controls
        devices.append(device)
    if len(devices) != count:
        raise ValueError("Device definition count mismatch")
    return {
        "sha256": hashlib.sha256(data).hexdigest(),
        "devices": devices,
        "evidence": "source_only",
        "wire_protocol_confirmed": False,
    }


def inspect_audio(data):
    """Inspect embedded XML data, without loading or running the executable."""
    devices = {}
    for match in re.finditer(rb'<device id="([^"<>]+)"', data):
        name = match[1].decode("utf-8")
        if not (name.startswith("Traktor Kontrol ")
                or name in {"Traktor MX2", "Traktor Z1 MK2"}):
            continue
        stop = data.find(b"</device>", match.end(), match.start() + 20000)
        if stop < 0:
            raise ValueError("Incomplete NI audio configuration")
        root = parse_xml(data[match.start():stop + len(b"</device>")])
        channels = {}
        for channel in root.findall("./routing/output/internal/channel"):
            key = channel.attrib["traktor"]
            if key in channels:
                raise ValueError("Duplicate output channel")
            channels[key] = int(channel.attrib["device"])
        keys = ["out_master_left", "out_master_right",
                "out_monitor_left", "out_monitor_right"]
        if any(key not in channels for key in keys):
            raise ValueError("Incomplete internal-mixer channel pairs")
        values = [channels[key] for key in keys]
        if any(v < 0 or v > 65535 for v in values) or len(set(values)) != 4:
            raise ValueError("Invalid internal-mixer channel pairs")
        item = {"device_name": name, "master": values[:2], "cue": values[2:]}
        driver = root.find("drivermapping")
        if driver is not None and driver.get("mac"):
            item["mac_driver_name"] = driver.get("mac")
        if name in devices and devices[name] != item:
            raise ValueError("Conflicting duplicate audio configuration")
        devices[name] = item
    if not devices:
        raise ValueError("No complete NI controller audio configuration found")
    return {"sha256": hashlib.sha256(data).hexdigest(),
            "devices": [devices[name] for name in sorted(devices)]}


def audit(factory, csi, executable):
    factory, csi = Path(factory), Path(csi)
    # Scan only vendor-owned NI mapping folders, never third-party mappings.
    roots = [factory / "Default Settings/Controller/Native Instruments",
             factory / "AutoConfig/Native Instruments"]
    if any(not root.is_dir() for root in roots) or not csi.is_dir():
        raise ValueError("Missing Traktor NI source directory")
    tsi = []
    for path in sorted(p for root in roots for p in root.rglob("*.tsi")):
        try:
            result = inspect_tsi(read_bounded(path, MAX_TSI))
        except ValueError as error:
            raise ValueError(f"{path.relative_to(factory)}: {error}") from error
        result["file"] = path.relative_to(factory).as_posix()
        tsi.append(result)
    if not tsi:
        raise ValueError("No NI controller TSI files found")
    qml = []
    for name in sorted(NI_CSI):
        directory = csi / name
        if directory.is_dir():
            sources = sorted(directory.rglob("*.qml"))
            qml.append({"device_directory": name, "qml_files": [
                {"file": p.relative_to(csi).as_posix(),
                 "sha256": hashlib.sha256(read_bounded(p, MAX_TSI)).hexdigest()}
                for p in sources]})
    return {"format": "ni_traktor_source_audit", "version": 1,
            "factory_version": factory.name, "tsi": tsi, "csi": qml,
            "audio": inspect_audio(read_bounded(executable, MAX_BINARY)),
            "hardware_verified": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    tsi = commands.add_parser("tsi", help="Inspect one controller TSI definition")
    tsi.add_argument("source", type=Path)
    tsi.add_argument("--controls", action="store_true")
    audio = commands.add_parser("audio", help="Inspect embedded NI audio XML")
    audio.add_argument("executable", type=Path)
    all_sources = commands.add_parser("audit", help="Audit installed NI sources")
    all_sources.add_argument("factory", type=Path)
    all_sources.add_argument("csi", type=Path)
    all_sources.add_argument("executable", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "tsi":
            result = inspect_tsi(read_bounded(args.source, MAX_TSI), args.controls)
        elif args.command == "audio":
            result = inspect_audio(read_bounded(args.executable, MAX_BINARY))
        else:
            result = audit(args.factory, args.csi, args.executable)
        print(json.dumps(result, indent=2, ensure_ascii=False))
    except (OSError, ValueError, KeyError, ET.ParseError) as error:
        print(f"Inspection failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
