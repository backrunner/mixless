#!/usr/bin/env python3
"""Export and plan all playlists from read-only, consistent library snapshots."""
import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess


def sha(path):
    with path.open('rb') as file:
        digest = hashlib.sha256()
        for chunk in iter(lambda: file.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def snapshot(path):
    return sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ('baseline', 'candidate', 'old-db', 'new-db', 'output'):
        parser.add_argument('--' + flag, type=Path, required=True)
    parser.add_argument('--baseline-revision', required=True)
    parser.add_argument('--candidate-revision', required=True)
    parser.add_argument('--workers', type=int, default=2)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Output directory must be new; preserve earlier evidence')
    old, new = snapshot(args.old_db), snapshot(args.new_db)
    # This audit harness currently supplies empty cues. Refuse to silently erase
    # user authority; use a cue-aware harness for such a snapshot.
    if any(db.execute('SELECT count(*) FROM cues WHERE user_set != 0').fetchone()[0] for db in (old, new)):
        parser.error('Snapshots contain manual cues; a cue-aware planner harness is required')
    inventory = 'SELECT playlist_id,position,track_id FROM playlist_items ORDER BY 1,2'
    if old.execute(inventory).fetchall() != new.execute(inventory).fetchall():
        parser.error('Old and new snapshots must contain identical playlist membership/order')
    hashes = 'SELECT id,content_hash FROM tracks ORDER BY id'
    if old.execute(hashes).fetchall() != new.execute(hashes).fetchall():
        parser.error('Old and new snapshots must describe the same audio content')
    args.output.mkdir(parents=True)
    output = args.output.resolve()
    analyses = {}
    for name, db in (('old', old), ('current', new)):
        analyses[name] = {tid: json.loads(payload) for tid, payload in
                          db.execute('SELECT track_id,payload FROM track_analysis')}
    (output / 'reference.json').write_text(json.dumps(list(analyses['current'].values())))
    sets = []
    for pid, title in old.execute('SELECT id,name FROM playlists ORDER BY id'):
        ids = [row[0] for row in old.execute(
            'SELECT track_id FROM playlist_items WHERE playlist_id=? ORDER BY position', (pid,))]
        if len(ids) < 2:
            continue
        sets.append({'id': pid, 'name': title, 'tracks': len(ids)})
        for name in analyses:
            (output / f'{name}-set{pid}.json').write_text(json.dumps([analyses[name][tid] for tid in ids]))
    runs = []
    for order in ('fixed', 'best'):
        for variant, source, binary in (('baseline', 'old', args.baseline.resolve()),
                                         ('analysis-only', 'current', args.baseline.resolve()),
                                         ('current', 'current', args.candidate.resolve())):
            for playlist in sets:
                pid = playlist['id']
                name = f'{variant}-{order}-set{pid}'
                runs.append({'name': name, 'set': pid, 'variant': variant, 'order': order,
                             'plans': name + '-plans.json', 'analysis': f'{source}-set{pid}.json',
                             'binary': str(binary)})
    manifest = {'reference_analyses': 'reference.json', 'sets': sets, 'runs': runs,
                'provenance': {'baseline': args.baseline_revision, 'candidate': args.candidate_revision,
                               'baseline_binary_sha256': sha(args.baseline),
                               'candidate_binary_sha256': sha(args.candidate),
                               'old_snapshot_sha256': sha(args.old_db), 'new_snapshot_sha256': sha(args.new_db),
                               'manual_cues': 0, 'dataset_role': 'regression; no holdout claim'}}
    (output / 'matrix.json').write_text(json.dumps(manifest, indent=2) + '\n')

    def run(spec):
        env = dict(os.environ, AUDIT_PLANS_OUT=str(output / spec['plans']),
                   AUDIT_ORDER_OUT=str(output / (spec['name'] + '-order.json')))
        command = [spec['binary'], str(output / spec['analysis']), '--stems', '--summary']
        if spec['order'] == 'best':
            command.append('--best')
        with (output / (spec['name'] + '.log')).open('w') as log:
            subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        return spec['name']

    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.workers)) as pool:
        for name in pool.map(run, runs):
            print(name, flush=True)


if __name__ == '__main__':
    main()
