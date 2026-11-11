"""Explicit playlist occurrences and cycle coverage for evaluation reports."""
from collections import Counter
from . import handoff, integrity


def annotate(result, plans, run):
    pairs = result['pairs']
    entries = run.get('queue_entries')
    size = run.get('cycle_size')
    if entries is not None:
        if len(entries) != len(plans) + 1:
            raise ValueError('Queue entries must include one entry per plan plus its successor')
        identities = {}
        for entry in entries:
            key = str(entry['entry_id'])
            if key in identities and identities[key] != entry['track_id']:
                raise ValueError('A playlist occurrence changed track identity')
            identities[key] = entry['track_id']
        for i, plan in enumerate(plans):
            if [entries[i]['track_id'], entries[i + 1]['track_id']] != plan['summary']['pair']:
                raise ValueError('Queue entries do not match plan adjacencies')
    if size is not None:
        if not isinstance(size, int) or size <= 0 or entries is None:
            raise ValueError('Cycle coverage requires a positive size and explicit queue entries')
        if len({str(e['entry_id']) for e in entries[:size]}) != size:
            raise ValueError('Distinct playlist occurrences require distinct entry IDs')
        for i, entry in enumerate(entries):
            if entry != entries[i % size]:
                raise ValueError('Cycle does not repeat the same playlist occurrences')
    for i, pair in enumerate(pairs):
        pair['occurrence'] = {'sequence_index': i,
                              'from_entry': str(entries[i]['entry_id']) if entries else None,
                              'to_entry': str(entries[i + 1]['entry_id']) if entries else None,
                              'cycle': i // size + 1 if size else None,
                              'index_in_cycle': i % size if size else None,
                              'wraparound': i % size == size - 1 if size else None}
        pair['carried_state'] = {key: plans[i][key] for key in
                                 ('t_in_a', 't_in_b', 'outgoing_offset', 'incoming_offset_end')}
    result['queue_coverage'] = {
        'identity_status': 'explicit' if entries else 'sequence_only',
        'cycle_size': size, 'evaluated_pairs': len(pairs), 'context_pairs': len(plans) - len(pairs),
        'departures': dict(Counter(str(e['entry_id']) for e in entries[:len(pairs)])) if entries else None}
    result['cycles'] = []
    if size:
        for offset in range(0, len(pairs), size):
            ps = pairs[offset:offset + size]
            result['cycles'].append({'cycle': offset // size + 1, 'pairs': len(ps),
                                     'complete': len(ps) == size,
                                     'handoff': handoff.aggregate(ps),
                                     'integrity': integrity.summarize(ps)})
    return result
