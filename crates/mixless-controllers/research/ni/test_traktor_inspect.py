"""Synthetic inspector tests. No NI source files or devices are required."""

import base64
import hashlib
from pathlib import Path
import struct
import tempfile
import unittest

import traktor_inspect as inspector


def number(value):
    return struct.pack(">I", value)


def chunk(tag, payload):
    return tag + number(len(payload)) + payload


def text(value):
    encoded = value.encode("utf-16-be")
    return number(len(encoded) // 2) + encoded


def table(tag, names):
    return chunk(tag, number(len(names)) + b"".join(
        chunk(b"DCDT", text(name) + b"opaque fields") for name in names))


def device(device_class):
    controls = table(b"DDCI", ["Probe.Input", "Probe.Encoder"]) + table(
        b"DDCO", ["Probe.Output"])
    # The serial/port and assignment data must never appear in output.
    definition = chunk(b"DDPT", text("PRIVATE-SERIAL-AND-PORT"))
    definition += chunk(b"DDDC", controls)
    definition += chunk(b"DDCB", b"PROPRIETARY-COMMAND-ASSIGNMENTS")
    definition += chunk(b"TEST", b"unknown optional metadata")
    return chunk(b"DEVI", text(device_class) + chunk(b"DDAT", definition))


def source(devices=None, count=None):
    if devices is None:
        devices = [device("Traktor.Offline Fixture")]
    payload = chunk(b"DIOM", chunk(b"DIOI", number(1)) + chunk(
        b"DEVS", number(len(devices) if count is None else count) + b"".join(devices)))
    encoded = base64.b64encode(payload).decode()
    xml = '<NIXML><Entry Name="DeviceIO.Config.Controller" Type="3" Value="{}"/></NIXML>'
    return xml.format(encoded).encode()


def audio(master=(0, 1), cue=(2, 3)):
    keys = ["out_master_left", "out_master_right", "out_monitor_left", "out_monitor_right"]
    pairs = ''.join(f'<channel traktor="{key}" device="{value}"/>'
                    for key, value in zip(keys, (*master, *cue)))
    return ('<device id="Traktor Kontrol Offline Fixture"><routing><output>'
            f'<internal>{pairs}</internal></output></routing></device>').encode()


class InspectorTests(unittest.TestCase):
    def test_native_class_controls_and_no_private_or_mapping_data(self):
        data = source()
        result = inspector.inspect_tsi(data)
        self.assertEqual(result["sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(result["devices"], [{"device_class": "Traktor.Offline Fixture",
                         "addressing": "native_symbols", "input_count": 2, "output_count": 1}])
        self.assertFalse(result["wire_protocol_confirmed"])
        self.assertNotIn("PRIVATE", repr(result))
        self.assertNotIn("PROPRIETARY", repr(result))
        self.assertNotIn("Probe", repr(result))
        expanded = inspector.inspect_tsi(data, include_controls=True)
        self.assertEqual(expanded["devices"][0]["controls"]["inputs"],
                         ["Probe.Input", "Probe.Encoder"])

    def test_multiple_device_classes_and_unresolved_class(self):
        result = inspector.inspect_tsi(source([device("Generic MIDI"), device("Unknown")]))
        self.assertEqual([d["addressing"] for d in result["devices"]],
                         ["generic_midi", "unresolved"])
        self.assertFalse(result["wire_protocol_confirmed"])

    def test_settings_only_does_not_imply_controller_definition(self):
        result = inspector.inspect_tsi(b'<NIXML><Entry Name="Offline.Settings"/></NIXML>')
        self.assertEqual(result["devices"], [])

    def test_reject_truncation_invalid_counts_and_container_lengths(self):
        for data in [source(count=2), source(count=300), b"<NIXML>",
                     source()[:-5]]:
            with self.subTest(data=data[:30]), self.assertRaises(ValueError):
                inspector.inspect_tsi(data)
        for payload in [b"DIOM\x00", chunk(b"DIOM", b"DEVS" + number(1000)),
                        chunk(b"DIOM", b"") + b"trailing"]:
            xml = b'<NIXML><Entry Name="DeviceIO.Config.Controller" Value="' + base64.b64encode(payload) + b'"/></NIXML>'
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                inspector.inspect_tsi(xml)

    def test_reject_entities_duplicate_entries_and_invalid_base64(self):
        for data in [b'<!DOCTYPE NIXML [<!ENTITY x "expanded">]><NIXML/>',
                     b'<NIXML><Entry Name="DeviceIO.Config.Controller" Value="!"/></NIXML>',
                     source().replace(b"</NIXML>", b'<Entry Name="DeviceIO.Config.Controller"/></NIXML>')]:
            with self.subTest(data=data[:30]), self.assertRaises(ValueError):
                inspector.inspect_tsi(data)

    def test_control_count_utf16_and_parent_boundaries(self):
        data = number(2) + chunk(b"DCDT", text("One"))
        with self.assertRaises(ValueError):
            inspector.descriptors(data, 0, len(data))
        data = number(1) + chunk(b"DCDT", number(100) + b"x")
        with self.assertRaises(ValueError):
            inspector.descriptors(data, 0, len(data))

    def test_audio_is_deduplicated_without_cross_generation_inference(self):
        data = b"binary\x00" + audio() + b"\x00" + audio()
        result = inspector.inspect_audio(data)
        self.assertEqual(result["devices"], [{"device_name": "Traktor Kontrol Offline Fixture",
                         "master": [0, 1], "cue": [2, 3]}])
        self.assertEqual(result["sha256"], hashlib.sha256(data).hexdigest())
        with self.assertRaises(ValueError):
            inspector.inspect_audio(audio() + audio(master=(4, 5)))

    def test_audio_invalid_missing_channels_and_non_ni_are_rejected(self):
        for data in [audio(cue=(0, 1)), audio(master=(-1, 1)),
                     audio().replace(b'out_monitor_right', b'missing'),
                     audio().replace(b'Traktor Kontrol Offline Fixture', b'Other Fixture'),
                     audio()[:-8]]:
            with self.subTest(data=data[:40]), self.assertRaises(ValueError):
                inspector.inspect_audio(data)

    def test_file_limit_and_audit_excludes_third_party_and_absolute_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            factory = root / "offline-version"
            ni = factory / "AutoConfig/Native Instruments"
            ni.mkdir(parents=True)
            (factory / "Default Settings/Controller/Native Instruments").mkdir(parents=True)
            (ni / "Fixture.tsi").write_bytes(source())
            other = factory / "AutoConfig/Other"
            other.mkdir()
            (other / "Never read.tsi").write_bytes(b"invalid")
            csi = root / "CSI"
            (csi / "MX2").mkdir(parents=True)
            (csi / "MX2/Fixture.qml").write_bytes(b"synthetic fixture")
            (csi / "Other").mkdir()
            executable = root / "binary"
            executable.write_bytes(audio())
            with self.assertRaises(ValueError):
                inspector.read_bounded(executable, 1)
            result = inspector.audit(factory, csi, executable)
            self.assertEqual(len(result["tsi"]), 1)
            self.assertEqual(len(result["csi"]), 1)
            self.assertEqual(result["tsi"][0]["file"], "AutoConfig/Native Instruments/Fixture.tsi")
            self.assertNotIn(directory, repr(result))
            self.assertFalse(result["hardware_verified"])


if __name__ == "__main__":
    unittest.main()
