"""Provenance guards for comparing snapshots, without real audio or a library."""
import json
from contextlib import closing
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / 'plan-automix-matrix.py'


class MatrixTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ('old.db', 'new.db'):
            db = sqlite3.connect(self.root / name)
            db.executescript('''
                CREATE TABLE cues(user_set INTEGER);
                CREATE TABLE playlists(id INTEGER, name TEXT);
                CREATE TABLE playlist_items(playlist_id INTEGER, position INTEGER, track_id INTEGER);
                CREATE TABLE tracks(id INTEGER, content_hash TEXT);
                CREATE TABLE track_analysis(track_id INTEGER, payload TEXT);
                INSERT INTO playlists VALUES (7, 'Synthetic');
                INSERT INTO playlist_items VALUES (7, 0, 1), (7, 1, 2);
                INSERT INTO tracks VALUES (1, 'one'), (2, 'two');
                INSERT INTO track_analysis VALUES (1, '{"track_id":1}'), (2, '{"track_id":2}');
            ''')
            db.close()
        self.binary = self.root / 'audit'
        self.binary.write_text('#!' + sys.executable + '\n'
                               'import os,pathlib,sys\n'
                               'pathlib.Path(os.environ["AUDIT_PLANS_OUT"]).write_text("[]")\n'
                               'print("best" if "--best" in sys.argv else "fixed")\n')
        self.binary.chmod(0o755)

    def run_matrix(self):
        return subprocess.run([sys.executable, str(SCRIPT), '--baseline', str(self.binary),
                               '--candidate', str(self.binary), '--old-db', str(self.root / 'old.db'),
                               '--new-db', str(self.root / 'new.db'), '--output', str(self.root / 'out'),
                               '--baseline-revision', 'old', '--candidate-revision', 'new'],
                              capture_output=True, text=True)

    def test_builds_all_six_fixed_and_best_controls(self):
        result = self.run_matrix()
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((self.root / 'out/matrix.json').read_text())
        self.assertEqual(len(manifest['runs']), 6)
        self.assertEqual(manifest['sets'][0]['tracks'], 2)
        for run in manifest['runs']:
            self.assertEqual((self.root / 'out' / (run['name'] + '.log')).read_text().strip(), run['order'])
        # Never overwrite an earlier evidence run.
        self.assertNotEqual(self.run_matrix().returncode, 0)

    def test_rejects_manual_cues_before_creating_evidence(self):
        with closing(sqlite3.connect(self.root / 'new.db')) as db, db:
            db.execute('INSERT INTO cues VALUES (1)')
        result = self.run_matrix()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('manual cues', result.stderr)
        self.assertFalse((self.root / 'out').exists())

    def test_rejects_different_audio_and_orders(self):
        with closing(sqlite3.connect(self.root / 'new.db')) as db, db:
            db.execute("UPDATE tracks SET content_hash='different' WHERE id=1")
        self.assertIn('same audio content', self.run_matrix().stderr)
        with closing(sqlite3.connect(self.root / 'new.db')) as db, db:
            db.execute('UPDATE playlist_items SET position=position+1')
        self.assertIn('identical playlist', self.run_matrix().stderr)


if __name__ == '__main__':
    unittest.main()
