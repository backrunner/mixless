"""Compare continuity diagnostics without treating proxy scores as listening truth."""
import math
from collections import Counter
from .metrics import VERSION


def signal_changes(before, after, step):
    """More problematic exposure requests review even if overlap grew too."""
    changes = []
    for name in sorted(set(before) | set(after)):
        a, b = before.get(name), after.get(name)
        if a is None or b is None:
            changes.append({'metric': name, 'reason': 'missing_metric'})
            continue
        for threshold in sorted(set(a['above_seconds']) | set(b['above_seconds'])):
            av, bv = a['above_seconds'].get(threshold), b['above_seconds'].get(threshold)
            if av is None or bv is None:
                changes.append({'metric': name, 'threshold': threshold, 'reason': 'missing_threshold'})
                continue
            if not all(math.isfinite(v) and v >= 0 for v in (av, bv)):
                raise ValueError(f'Invalid signal exposure: {name}')
            if bv - av > step + 1e-8:
                changes.append({'metric': name, 'threshold': threshold,
                                'before': av, 'after': bv, 'reason': 'increased_exposure'})
        unknown = lambda m: max(0., m['eligible_seconds'] - m['observed_seconds'])
        if unknown(b) - unknown(a) > step + 1e-8:
            changes.append({'metric': name, 'before': unknown(a), 'after': unknown(b),
                            'reason': 'increased_unknown_exposure'})
    return changes


def compare(before, after, step):
    old, new = before['summary'], after['summary']
    if old['pairs'] != new['pairs'] or len(before['pairs']) != len(after['pairs']):
        raise ValueError('Transition counts differ; compare the same cycle coverage')
    if any(r['summary']['pairs'] != len(r['pairs']) for r in (before, after)):
        raise ValueError('Summary and transition counts disagree')
    for side in (0, 1):
        coverage = lambda r: Counter(p['pair'][side] for p in r['pairs'])
        if coverage(before) != coverage(after):
            raise ValueError('Track occurrence coverage differs')
    qa, qb = before.get('queue_coverage', {}), after.get('queue_coverage', {})
    for key in ('identity_status', 'cycle_size', 'evaluated_pairs', 'context_pairs', 'departures'):
        if qa.get(key) != qb.get(key):
            raise ValueError(f'Queue coverage differs: {key}')
    risk_fields = ('vocal_damage_pairs',
                   'foreground_gap_seconds', 'abrupt_tonal_loss_pairs',
                   'unavailable_pairs', 'partial_pairs', 'unknown_phrase_source_seconds')
    fields = {'violation_pairs': (old['violation_pairs'], new['violation_pairs'])}
    for key in risk_fields:
        fields[key] = (old['integrity'][key], new['integrity'][key])
    for key in ('overlap_unknown_seconds', 'unknown_heard_endpoint_pairs', 'unavailable_pairs'):
        fields['handoff_' + key] = (old['handoff'][key], new['handoff'][key])
    for prefix, a, b in [('integrity', old['integrity'], new['integrity'])]:
        fields[prefix + '_unknown_seconds'] = tuple(max(0., m['eligible_seconds'] - m['observed_seconds']) for m in (a, b))
    alerts, measurements = [], {}
    for key, (a, b) in fields.items():
        if not all(isinstance(v, (int, float)) and math.isfinite(v) and v >= 0 for v in (a, b)):
            raise ValueError(f'Invalid measurement: {key}')
        measurements[key] = {'before': a, 'after': b, 'delta': b - a}
        if b - a > (step + 1e-8 if key.endswith('seconds') else 0):
            alerts.append(key)
    # Arrangement and legacy threshold counts describe changes, not quality.
    descriptors = {key: {'before': old['integrity'][key], 'after': new['integrity'][key]}
                   for key in ('instrument_bed_gap_seconds', 'voice_carried_bed_gap_seconds',
                               'sustained_instrument_risk_pairs')}
    descriptors['handoff'] = {'before': old['handoff'], 'after': new['handoff']}
    sensitivity = {}
    for pulses in ('1', '2', '3'):
        a = before.get('vocal_grouping_sensitivity', {}).get(pulses)
        b = after.get('vocal_grouping_sensitivity', {}).get(pulses)
        if a is None or b is None:
            alerts.append(f'vocal_grouping_{pulses}_unavailable')
            continue
        sensitivity[pulses] = {'before': a['vocal_damage_pairs'], 'after': b['vocal_damage_pairs']}
        if b['vocal_damage_pairs'] > a['vocal_damage_pairs']:
            alerts.append(f'vocal_grouping_{pulses}')
        if b['unavailable_pairs'] + b.get('partial_pairs', 0) > a['unavailable_pairs'] + a.get('partial_pairs', 0):
            alerts.append(f'vocal_grouping_{pulses}_coverage')
    signals = signal_changes(old.get('metrics') or {}, new.get('metrics') or {}, step)
    if signals:
        alerts.append('signal_exposure')
    same = [p['pair'] for p in before['pairs']] == [p['pair'] for p in after['pairs']]
    same_occurrences = same and [p.get('occurrence') for p in before['pairs']] == [p.get('occurrence') for p in after['pairs']]
    occurrences = []
    if same_occurrences:
        for i, (a, b) in enumerate(zip(before['pairs'], after['pairs'])):
            changes = signal_changes(a.get('metrics') or {}, b.get('metrics') or {}, step)
            ai, bi = a['integrity'], b['integrity']
            for key in ('foreground_gap_seconds', 'abrupt_tonal_loss', 'unknown_phrase_source_seconds'):
                if key in ai and key in bi and bi[key] - ai[key] > step + 1e-8:
                    changes.append({'metric': key, 'before': ai[key], 'after': bi[key], 'reason': 'increased_exposure'})
            for key in ('vocal_events',):
                av, bv = ai.get(key, []), bi.get(key, [])
                severity = lambda es: max((e['lost_energy_fraction'] for e in es), default=0.)
                if len(bv) > len(av) or severity(bv) > severity(av) + 1e-8:
                    changes.append({'metric': key, 'before': av, 'after': bv, 'reason': 'increased_phrase_risk'})
            # Overlapping pitches are not a sentence. Compare individual
            # heard-note changes for review; never call a pad fade phrase damage.
            av, bv = ai.get('instrument_note_events', []), bi.get('instrument_note_events', [])
            identity = lambda e: (e['side'], e['source_start'], e['source_end'], e['midi'])
            prior = {identity(e): e for e in av}
            changed = [e for e in bv if identity(e) not in prior
                       or any(e[k] and not prior[identity(e)][k]
                              for k in ('hidden_attack', 'attenuated_release'))
                       or e['max_power_gain_change_per_pulse'] >
                       prior[identity(e)]['max_power_gain_change_per_pulse'] + 1e-6]
            if changed:
                changes.append({'metric': 'instrument_note_events', 'before': av,
                                'after': changed, 'reason': 'note_control_change_requires_review'})
            if bi.get('unknown_instrument_notes', 0) > ai.get('unknown_instrument_notes', 0):
                changes.append({'metric': 'instrument_note_coverage', 'reason': 'increased_unknown_notes'})
            for pulses in ('1', '2', '3'):
                av = a.get('vocal_grouping', {}).get(pulses)
                bv = b.get('vocal_grouping', {}).get(pulses)
                if av is None or bv is None:
                    changes.append({'metric': 'vocal_grouping_' + pulses, 'reason': 'missing_pair_sensitivity'})
                    continue
                ae, be = av.get('vocal_events') or [], bv.get('vocal_events') or []
                if (len(be) > len(ae) or severity(be) > severity(ae) + 1e-8
                        or av['status'] == 'observed' and bv['status'] != 'observed'):
                    changes.append({'metric': 'vocal_grouping_' + pulses,
                                    'before': av, 'after': bv, 'reason': 'increased_phrase_risk_or_unknown'})
            if ai['status'] == 'observed' and bi['status'] != 'observed':
                changes.append({'metric': 'integrity_coverage', 'reason': 'lost_observation'})
            ah, bh = a['handoff'], b['handoff']
            au = ah.get('overlap', {}).get('unknown_seconds', 0.)
            bu = bh.get('overlap', {}).get('unknown_seconds', 0.)
            if bu - au > step + 1e-8:
                changes.append({'metric': 'handoff_coverage', 'before': au, 'after': bu,
                                'reason': 'lost_observation'})
            if changes:
                occurrences.append({'index': i, 'pair': a['pair'], 'changes': changes})
    if occurrences:
        alerts.append('pair_exposure')
    # Counts of deviating transitions and the distance to published DJ shares.
    # Reports written before this block existed compare as unavailable.
    realism = None
    ra, rb = before.get('realism'), after.get('realism')
    if ra and rb and ra['version'] == rb['version']:
        realism = {'levels': {level: {key: {'before': ra['levels'][level][key], 'after': rb['levels'][level][key]}
                                      for key in ('clean', 'deviating', 'unknown')} for level in ra['levels']},
                   'dj_reference_gap_points': {'before': ra['dj_reference'].get('mean_gap_percentage_points'),
                                               'after': rb['dj_reference'].get('mean_gap_percentage_points')}}
        realism['score'] = {'before': ra['score'], 'after': rb['score']}
        realism['unscored_transitions'] = {'before': ra['unscored_transitions'], 'after': rb['unscored_transitions']}
        if any(v['deviating']['after'] > v['deviating']['before'] for v in realism['levels'].values()):
            alerts.append('realism_deviating_transitions')
        if ra['score'] is not None and (rb['score'] is None or rb['score'] < ra['score'] - 1e-6):
            alerts.append('realism_score')
    incomplete = (any(new['integrity'][k] for k in ('partial_pairs', 'unavailable_pairs'))
                  or any(new['handoff'][k] for k in ('unavailable_pairs', 'unknown_heard_endpoint_pairs'))
                  or new['handoff']['overlap_unknown_seconds'] > step
                  or any(m['eligible_seconds'] - m['observed_seconds'] > step
                         for m in (new.get('metrics') or {}).values()))
    def terminals(run):
        return [{'index': i, 'pair': p['pair'], 'occurrence': p.get('occurrence'),
                 'handoff': p['handoff'], 'integrity': p['integrity']}
                for i, p in enumerate(run['pairs']) if p['handoff']['legacy_tail_short'] is True]
    return {
        'status': 'review_required' if alerts else 'incomplete_evidence' if incomplete else 'no_regression_detected',
        'same_adjacencies': same,
        'same_occurrences': same_occurrences if qa.get('identity_status') == 'explicit' else None,
        'pair_alignment': 'playlist_occurrences' if qa.get('identity_status') == 'explicit' else 'sequence_only',
        'same_carried_states': same_occurrences and [p.get('carried_state') for p in before['pairs']] == [p.get('carried_state') for p in after['pairs']],
        'pairs': new['pairs'], 'measurements': measurements, 'descriptors': descriptors, 'alerts': alerts,
        'signal_review': signals, 'pair_review': occurrences, 'realism': realism,
        'pitch': {key: {'before': old.get(key), 'after': new.get(key)}
                  for key in ('max_abs_pitch', 'mean_abs_pitch_semitones', 'pitch_observed_deck_seconds')},
        'vocal_grouping_sensitivity': sensitivity,
        'audibility_sensitivity': {'before': before.get('audibility_sensitivity'), 'after': after.get('audibility_sensitivity')},
        'terminal_before': terminals(before), 'terminal_after': terminals(after),
        'listening_acceptance': 'not_assessed',
    }


def compare_reports(before, after, analysis_hash, before_run=None, after_run=None):
    if before['observer_version'] != VERSION or after['observer_version'] != VERSION:
        raise ValueError('Re-evaluate both reports with the current observer version')
    for key in ('observer_version', 'step_seconds', 'normalization'):
        if before[key] != after[key]:
            raise ValueError(f'Observer settings differ: {key}')
    if not math.isfinite(before['step_seconds']) or before['step_seconds'] <= 0:
        raise ValueError('Invalid observer resolution')
    for report in (before, after):
        if analysis_hash not in report['input_sha256'].values():
            raise ValueError('Report was not evaluated against the supplied reference analysis')
    def select(report, name):
        runs = report['runs']
        matching = runs if name is None else [r for r in runs if r['name'] == name]
        if len(matching) != 1:
            raise ValueError('Select exactly one named run from each report')
        return matching[0]
    return compare(select(before, before_run), select(after, after_run), before['step_seconds'])
