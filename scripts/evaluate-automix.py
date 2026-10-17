#!/usr/bin/env python3
"""Evaluate stored plans against a common reference without opening a library."""
import argparse
import hashlib
import json
from pathlib import Path

from automix_eval.metrics import VERSION, STEP, cue_events, evaluate_set
from automix_eval.signal import Track


def read(path, inputs):
    data = Path(path).read_bytes()
    inputs[str(path)] = hashlib.sha256(data).hexdigest()
    return json.loads(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--analyses', type=Path)
    parser.add_argument('--plans', type=Path)
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--sensitivity', action='store_true', help='Also evaluate 1%% and 5%% audibility')
    args = parser.parse_args()
    inputs = {}
    if args.manifest:
        manifest = read(args.manifest, inputs)
        root = args.manifest.parent
        analysis_path = root / manifest['reference_analyses']
        runs = manifest['runs']
    elif args.analyses and args.plans:
        analysis_path = args.analyses
        root = Path('.')
        manifest = {}
        runs = [{'name': 'single', 'plans': str(args.plans)}]
    else:
        parser.error('Supply --manifest, or both --analyses and --plans')
    analyses = read(analysis_path, inputs)
    tracks = {t['track_id']: Track(t) for t in analyses}
    if len(tracks) != len(analyses):
        raise ValueError('Reference analysis contains duplicate track IDs')
    output = {'observer_version': VERSION, 'step_seconds': STEP,
              'normalization': 'per_track_active_p75_band_power',
              'provenance': manifest.get('provenance'), 'runs': []}
    for run in runs:
        plans = read(root / run['plans'], inputs)
        result = {k: v for k, v in run.items() if k != 'plans'}
        result.update(evaluate_set(plans, tracks))
        if args.sensitivity:
            result['audibility_sensitivity'] = {
                str(cutoff): evaluate_set(plans, tracks, cutoff)['summary'] for cutoff in (.01, .05)}
        if 'annotations' in manifest and 'cues' in run:
            result['cue_events'] = cue_events(read(root / manifest['annotations'], inputs),
                                              read(root / run['cues'], inputs))
        else:
            result['cue_events'] = cue_events(None, None)
        output['runs'].append(result)
        print(run['name'], json.dumps(result['summary'], separators=(',', ':')), flush=True)
    output['input_sha256'] = inputs
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, allow_nan=False) + '\n')


if __name__ == '__main__':
    main()
