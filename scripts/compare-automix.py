#!/usr/bin/env python3
"""Compare fixed queues and fresh Best separately across named playlists."""
import argparse
import hashlib
import json
from pathlib import Path

from automix_eval.comparison import compare_reports


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = args.manifest.parent
    manifest = json.loads(args.manifest.read_text())
    results = []
    inputs = {}
    def fingerprint(path):
        if path not in inputs:
            with (root / path).open('rb') as source:
                digest = hashlib.sha256()
                for block in iter(lambda: source.read(1024 * 1024), b''):
                    digest.update(block)
                inputs[path] = digest.hexdigest()
        return inputs[path]

    def read(path):
        data = (root / path).read_bytes()
        inputs[path] = hashlib.sha256(data).hexdigest()
        return json.loads(data)
    names = set()
    for case in manifest['comparisons']:
        if case['name'] in names:
            raise ValueError('Comparison names must be unique')
        names.add(case['name'])
        fingerprint(case['analyses'])
        result = compare_reports(read(case['baseline']), read(case['candidate']),
                                 inputs[case['analyses']], case.get('baseline_run'), case.get('candidate_run'))
        results.append({'name': case['name'], 'sources': case, **result})
    if not results:
        raise ValueError('No comparisons supplied')
    output = {'comparisons': results, 'input_sha256': inputs,
              'manifest_sha256': hashlib.sha256(args.manifest.read_bytes()).hexdigest(),
              'review_required': any(r['status'] != 'no_regression_detected' for r in results),
              'listening_acceptance': 'not_assessed'}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, allow_nan=False) + '\n')
    print(json.dumps({'comparisons': len(results), 'review_required': output['review_required']}))


if __name__ == '__main__':
    main()
