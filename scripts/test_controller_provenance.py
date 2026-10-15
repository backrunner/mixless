"""Synthetic regression tests for the limited source/artifact guard."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("provenance_guard", Path(__file__).with_name("check-controller-provenance.py"))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class GuardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.files = []
        self.write("crates/mixless-controllers/CLEAN_ROOM_REVIEW.md", "Synthetic record")
        self.write("crates/mixless-controllers/NI_PROVENANCE.json", json.dumps({
            "implementation_status": "independent_implementation", "strict_clean_room_verified": False,
            "legal_clearance": "not_assessed",
            "exposure": {"research_and_implementation_separated": False}}))

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content if isinstance(content, bytes) else content.encode())
        self.files.append(name)

    def test_own_code_and_factual_metadata_are_allowed(self):
        self.write("crates/mixless-controllers/src/own.rs", "fn independent() {}")
        self.write("crates/mixless-controllers/research/ni/traktor-4.4.1-audit.json", json.dumps({
            "hardware_verified": False, "tsi": [{"file": "Vendor.tsi", "sha256": "synthetic",
            "wire_protocol_confirmed": False, "devices": []}]}))
        self.assertEqual(guard.check(self.root, self.files), [])

    def test_original_formats_and_native_binaries_are_rejected(self):
        for name, data in [("Vendor.qml", b"vendor"), ("Vendor.tsi", b"vendor"),
                           ("Template.nckf1", b"vendor"), ("Template.ncm2", b"vendor"),
                           ("renamed.bin", b"\xcf\xfa\xed\xfe")]:
            with self.subTest(name=name):
                filename = "crates/mixless-controllers/" + name
                self.write(filename, data)
                self.assertTrue(guard.check(self.root, self.files))
                (self.root / filename).unlink()
                self.files.remove(filename)

    def test_vendor_loading_and_embedded_source_payloads_are_rejected(self):
        self.write("crates/mixless-controllers/src/loader.rs", '#[link(name="NHLDriver")] extern "C" {}')
        self.assertTrue(guard.check(self.root, self.files))
        self.files.pop()
        self.write("crates/mixless-controllers/research/ni/traktor-4.4.1-audit.json", '{"payload":"original bytes"}')
        self.assertTrue(guard.check(self.root, self.files))

    def test_false_clean_room_and_protocol_claims_are_rejected(self):
        path = self.root / "crates/mixless-controllers/NI_PROVENANCE.json"
        record = json.loads(path.read_text())
        record["strict_clean_room_verified"] = True
        path.write_text(json.dumps(record))
        self.assertTrue(guard.check(self.root, self.files))
        record["strict_clean_room_verified"] = False
        record["exposure"]["research_and_implementation_separated"] = True
        path.write_text(json.dumps(record))
        self.assertTrue(guard.check(self.root, self.files))
        record["exposure"]["research_and_implementation_separated"] = False
        path.write_text(json.dumps(record))
        self.write("crates/mixless-controllers/research/ni/traktor-4.4.1-audit.json", '{"wire_protocol_confirmed":true}')
        self.assertTrue(guard.check(self.root, self.files))

    def test_deleted_required_record_is_rejected(self):
        (self.root / "crates/mixless-controllers/CLEAN_ROOM_REVIEW.md").unlink()
        self.assertTrue(guard.check(self.root, self.files))

    def test_controller_symlink_is_rejected_without_reading_its_target(self):
        name = "crates/mixless-controllers/src/vendor-link.rs"
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.symlink_to(self.root / "nonexistent-external-source")
        self.files.append(name)
        self.assertTrue(guard.check(self.root, self.files))


if __name__ == "__main__":
    unittest.main()
