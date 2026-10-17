"""Versioned diagnostics with exposure, coverage and counterfactual references."""
import math
from .signal import Plan, grid_error, heard_chroma, roughness

VERSION = 1
STEP = .05


class Measure:
    def __init__(self, thresholds=()):
        self.values = []
        self.eligible = 0.0
        self.thresholds = thresholds

    def add(self, value, seconds=STEP, weight=1):
        self.eligible += seconds
        if value is not None:
            self.values.append((value, seconds, weight))

    def report(self):
        observed = sum(s for _, s, _ in self.values)
        weighted = sum(s * w for _, s, w in self.values)
        ordered = sorted(self.values)
        cumulative, p95 = 0, None
        for v, s, _ in ordered:
            cumulative += s
            if cumulative >= observed * .95:
                p95 = v
                break
        return {'eligible_seconds': self.eligible, 'observed_seconds': observed,
                'mean': sum(v * s * w for v, s, w in self.values) / weighted if weighted else None,
                'p95': p95, 'max': max((v for v, _, _ in self.values), default=None),
                'above_seconds': {str(t): sum(s for v, s, _ in self.values if v > t)
                                  for t in self.thresholds}}


def total(p):
    return sum(sum(band) for band in p) if p else 0


def tonal(p):
    return sum(p[0][1:]) + sum(p[2][1:]) if p else 0


def rhythmic(d):
    p, raw = d['heard'], d['raw']
    if not p or not raw:
        return 0
    return sum(p[1]) + (sum(p[2]) + sum(p[3]) if raw['rhythmic_instruments'] else 0)


def db_ratio(a, b):
    return 10 * math.log10(max(a, 1e-12) / max(b, 1e-12))


def voice_episodes(rows, side):
    """Only score phrases that are actually exposed, including quiet held notes."""
    episodes, current = [], []
    for row in rows:
        d = row['decks'][side]
        if d['raw'] and d['raw']['voice']:
            current.append(row)
        elif current:
            episodes.append(current)
            current = []
    if current:
        episodes.append(current)
    return [ep for ep in episodes if any(
        sum(r['decks'][side]['heard'][0]) > .002 for r in ep if r['decks'][side]['heard'])]


def evaluate_pair(data, tracks, previous=None, audible=.02):
    plan = Plan(data, tracks)
    duration = plan.duration
    metrics = {
        'grid_phase_ms': Measure((20, 30, 50)),
        'grid_period_fraction': Measure((.005, .01, .02)),
        'drum_grid_phase_ms': Measure((20, 30, 50)),
        'drum_grid_period_fraction': Measure((.005, .01, .02)),
        'drum_activity_change': Measure((.1, .25, .5)),
        'tonal_roughness': Measure((.25, .4, .55)),
        'voice_attenuation_db': Measure((3, 6, 9)),
        'peak_deficit_db': Measure((3, 6, 9)),
        'foreground_competition': Measure((.05, .1, .2)),
        'tonal_control_rise_per_second': Measure((1, 2, 4)),
        'pitch_motion_voice_exposure': Measure((.01, .1, .5)),
    }
    result = {'pair': data['summary']['pair'], 'strategy': data['summary']['strategy'],
              'plan_seconds': duration, 'reported_bars': data['summary']['length_bars'],
              'cut': data['summary']['length_bars'] == 0,
              'fallback': bool(data['summary']['used_fallback']),
              'unsupported': plan.unsupported,
              'dry_only_fx': bool(data.get('send_fx')),
              'audible_overlap_seconds': 0.0, 'audible_overlap_bars': 0.0,
              'stem_observed_deck_seconds': 0.0, 'stem_eligible_deck_seconds': 0.0,
              'pitch_semitone_seconds': 0.0, 'pitch_observed_seconds': 0.0,
              'max_abs_pitch': 0.0, 'voice_exit_crossing': None, 'voice_entry_crossing': None,
              'violations': []}
    if previous:
        if previous['summary']['pair'][1] != plan.a.id:
            result['violations'].append('nonsequential_pair')
        if data['t_in_a'] + .05 < previous['t_end_b']:
            result['violations'].append('overlapping_transitions')
        for k in ('rate', 'pitch_semitones'):
            tolerance = .002 if k == 'rate' else .02
            if abs(data['outgoing_offset'][k] - previous['incoming_offset_end'][k]) > tolerance:
                result['violations'].append('carried_' + k)
    for track, begin, end in ((plan.a, data['t_in_a'], data['t_out_a']),
                              (plan.b, data['t_in_b'], data['t_end_b'])):
        if begin < -.05 or end > track.duration + .1 or end < begin:
            result['violations'].append('source_bounds')
    # Missing/different DSP means unavailable, not an artificially perfect score.
    rows = []
    if not plan.unsupported:
        count = math.ceil((duration + 2) / STEP)
        for i in range(count):
            wall = -1 + (i + .5) * STEP
            dt = min(STEP, duration + 1 - (wall - STEP / 2))
            u, decks = plan.at(wall)
            rows.append({'wall': wall, 'dt': dt, 'u': u, 'decks': decks})
        for i, row in enumerate(rows):
            wall, dt, decks = row['wall'], row['dt'], row['decks']
            a, b = decks
            inside = 0 <= wall < duration
            observed = all(d['raw'] is not None for d in decks if d['playing'])
            if inside:
                for d in decks:
                    if d['playing']:
                        result['stem_eligible_deck_seconds'] += dt
                        result['stem_observed_deck_seconds'] += dt * (d['raw'] is not None)
                if all(d['raw'] for d in decks) and min(total(d['heard']) for d in decks) > audible:
                    result['audible_overlap_seconds'] += dt
                    du = plan.inverse.at(min(duration, wall + dt / 2)) - plan.inverse.at(max(0, wall - dt / 2))
                    result['audible_overlap_bars'] += du
                for d in decks:
                    if total(d['heard']) > audible:
                        result['pitch_semitone_seconds'] += abs(d['pitch']) * dt
                        result['pitch_observed_seconds'] += dt
                        result['max_abs_pitch'] = max(result['max_abs_pitch'], abs(d['pitch']))
                if min(rhythmic(d) for d in decks) > audible:
                    ga = plan.a.grid(a['source'], a['rate'])
                    gb = plan.b.grid(b['source'], b['rate'])
                    error = grid_error(ga, gb) if ga and gb else (None, None)
                    metrics['grid_period_fraction'].add(error[0], dt)
                    metrics['grid_phase_ms'].add(error[1], dt)
                    if min(sum(d['heard'][1]) for d in decks) > audible:
                        metrics['drum_grid_period_fraction'].add(error[0], dt)
                        metrics['drum_grid_phase_ms'].add(error[1], dt)
                ta, tb = tonal(a['heard']), tonal(b['heard'])
                harmonic_power = [sum(sum(d['heard'][part]) for part in (0, 2, 3))
                                  if d['heard'] else 0 for d in decks]
                if min(harmonic_power) > audible:
                    ca, cb = heard_chroma(a), heard_chroma(b)
                    metrics['tonal_roughness'].add(roughness(ca, cb) if ca and cb else None, dt, min(harmonic_power))
                if observed and min(total(d['heard']) for d in decks) > audible:
                    metrics['foreground_competition'].add(min(ta, tb), dt)
                # Audible pitch movement is assessed separately from the benefit
                # of reducing long-term offset. A muted deck does not bend a voice.
                if i:
                    for side, d in enumerate(decks):
                        if d['raw'] and d['raw']['voice'] and total(d['heard']) > audible:
                            velocity = abs(d['pitch'] - rows[i - 1]['decks'][side]['pitch']) / dt
                            metrics['pitch_motion_voice_exposure'].add(velocity * math.sqrt(sum(d['heard'][0])), dt)
            # Include one second after handoff: a cut cannot evade energy metrics.
            if 0 <= wall < plan.mix_end + 1:
                window = rows[max(0, i - 19):i + 1]
                known = all(all(d['raw'] for d in r['decks']) for r in window)
                outgoing = sum(total(r['decks'][0]['raw']['power']) for r in window if r['decks'][0]['raw']) / len(window)
                if outgoing >= .5:
                    if known:
                        available = sum(max(total(r['decks'][0]['raw']['power']),
                                            total(r['decks'][1]['raw']['power']) if r['decks'][1]['playing'] else 0)
                                        for r in window) / len(window)
                        heard = sum(sum(total(d['heard']) for d in r['decks']) for r in window) / len(window)
                        metrics['peak_deficit_db'].add(max(0, db_ratio(available, heard)), dt)
                    else:
                        metrics['peak_deficit_db'].add(None, dt)
                if i >= 5 and observed:
                    rises = []
                    for side, d in enumerate(decks):
                        if not d['raw']:
                            continue
                        old = rows[i - 5]['decks'][side]
                        delta = sum(d['raw']['power'][part][band] * max(0,
                            math.sqrt(d['gains'][part][band]) - math.sqrt(old['gains'][part][band])) ** 2
                                    for part in (0, 2) for band in (1, 2))
                        rises.append(math.sqrt(delta) / .25)
                    metrics['tonal_control_rise_per_second'].add(max(rises, default=0), dt)
        for ep in voice_episodes(rows, 0):
            heard_until = max(r['wall'] for r in ep if r['decks'][0]['heard']
                              and sum(r['decks'][0]['heard'][0]) > .002) + STEP
            for row in ep:
                if not 0 <= row['wall'] < min(plan.mix_end, heard_until):
                    continue
                d = row['decks'][0]
                raw = sum(d['raw']['power'][0])
                heard = sum(d['heard'][0])
                if raw > 1e-10:
                    metrics['voice_attenuation_db'].add(max(0, db_ratio(raw, heard)), row['dt'])
        activity = []
        for row in rows:
            known = all(d['raw'] for d in row['decks'] if d['playing'])
            activity.append(sum(d['raw']['drum_onset'] * math.sqrt(sum(d['heard'][1]))
                                for d in row['decks'] if d['playing'] and d['raw']) if known else None)
        for i, row in enumerate(rows):
            if not 0 <= row['wall'] <= plan.mix_end + STEP or i < 20 or i + 20 > len(rows):
                continue
            before, after = activity[i - 20:i], activity[i:i + 20]
            change = abs(sum(after) - sum(before)) / 20 if all(v is not None for v in before + after) else None
            metrics['drum_activity_change'].add(change, row['dt'])
        # Notes spanning the last/first audible voice are explicit truncation
        # diagnostics. A detector without notes cannot certify a clean boundary.
        for side, track, key in ((0, plan.a, 'voice_exit_crossing'), (1, plan.b, 'voice_entry_crossing')):
            audible_rows = [r for r in rows if r['decks'][side]['heard']
                            and sum(r['decks'][side]['heard'][0]) > .002]
            if track.notes and audible_rows:
                edge = (audible_rows[-1] if side == 0 else audible_rows[0])['decks'][side]['source']
                result[key] = any(n['start_sec'] + .05 < edge < n['end_sec'] - .05 for n in track.notes)
    result['metrics'] = {k: v.report() for k, v in metrics.items()}
    result['played_out'] = (data['t_out_a'] >= min(plan.a.duration * .92, plan.a.duration - 8)
                            and result['audible_overlap_bars'] < 4)
    entry = previous['t_in_b'] if previous else 0
    drops = [s for s in plan.a.data['sections'] if s['label'] == 'drop'
             and s['start_sec'] >= entry and s['end_sec'] <= data['t_out_a']]
    result['complete_drop_count'] = len(drops)
    result['post_first_drop_seconds'] = data['t_out_a'] - drops[0]['end_sec'] if drops else None
    return result


def evaluate_set(plans, tracks, audible=.02):
    pairs = [evaluate_pair(p, tracks, plans[i - 1] if i else None, audible)
             for i, p in enumerate(plans)]
    # Include every solo stretch exactly once, including the final track. Plan
    # curves include any committed solo restoration; remaining gaps hold offsets.
    solo_burden, solo_seconds, solo_observed, solo_audible, max_pitch = 0., 0., 0., 0., 0.
    if plans:
        first = tracks[plans[0]['summary']['pair'][0]]
        offset = plans[0]['outgoing_offset']
        rate = max(.01, offset['rate'])
        for frame, bands in zip(first.frames, first.powers):
            seconds = max(0, min(plans[0]['t_in_a'], frame['end_sec']) - frame['start_sec']) / rate
            solo_observed += seconds
            if total(bands) > audible:
                solo_audible += seconds
                solo_burden += seconds * abs(offset['pitch_semitones'])
        solo_seconds += max(0, plans[0]['t_in_a']) / rate
    for i, p in enumerate(plans):
        track = tracks[p['summary']['pair'][1]]
        end = plans[i + 1]['t_in_a'] if i + 1 < len(plans) else track.duration
        offset = p['incoming_offset_end']
        rate = max(.01, offset['rate'])
        seconds = max(0, end - p['t_end_b']) / rate
        audible_seconds = 0.
        for frame, bands in zip(track.frames, track.powers):
            covered = max(0, min(end, frame['end_sec']) - max(p['t_end_b'], frame['start_sec'])) / rate
            solo_observed += covered
            if total(bands) > audible:
                audible_seconds += covered
        solo_burden += audible_seconds * abs(offset['pitch_semitones'])
        solo_audible += audible_seconds
        solo_seconds += seconds
        max_pitch = max(max_pitch, abs(offset['pitch_semitones']))
    summary = {'pairs': len(pairs), 'cuts': sum(p['cut'] for p in pairs),
               'fallbacks': sum(p['fallback'] for p in pairs),
               'played_out': sum(p['played_out'] for p in pairs),
               'audible_overlap_ge4_bars': sum(p['audible_overlap_bars'] >= 4 for p in pairs),
               'unsupported_pairs': sum(bool(p['unsupported']) for p in pairs),
               'dry_fx_pairs': sum(p['dry_only_fx'] for p in pairs),
               'violation_pairs': sum(bool(p['violations']) for p in pairs),
               'solo_pitch_semitone_seconds': solo_burden, 'solo_seconds': solo_seconds,
               'solo_observed_seconds': solo_observed,
               'pitch_semitone_seconds': solo_burden + sum(p['pitch_semitone_seconds'] for p in pairs),
               'pitch_observed_deck_seconds': solo_audible + sum(p['pitch_observed_seconds'] for p in pairs),
               'max_abs_pitch': max([max_pitch] + [p['max_abs_pitch'] for p in pairs])}
    for key in ('plan_seconds', 'audible_overlap_seconds', 'stem_observed_deck_seconds', 'stem_eligible_deck_seconds'):
        summary[key] = sum(p[key] for p in pairs)
    summary['mean_abs_pitch_semitones'] = (summary['pitch_semitone_seconds'] /
                                           summary['pitch_observed_deck_seconds']
                                           if summary['pitch_observed_deck_seconds'] else None)
    for key in ('voice_exit_crossing', 'voice_entry_crossing'):
        summary[key] = {'observed_pairs': sum(p[key] is not None for p in pairs),
                        'crossing_pairs': sum(p[key] is True for p in pairs)}
    delays = [p['post_first_drop_seconds'] for p in pairs if p['post_first_drop_seconds'] is not None]
    summary['progression'] = {'first_drop_observed_pairs': len(delays),
                              'mean_post_first_drop_seconds': sum(delays) / len(delays) if delays else None,
                              'multiple_complete_drop_pairs': sum(p['complete_drop_count'] > 1 for p in pairs)}
    summary['metrics'] = {}
    for key in pairs[0]['metrics'] if pairs else []:
        measures = [p['metrics'][key] for p in pairs]
        observed = sum(m['observed_seconds'] for m in measures)
        summary['metrics'][key] = {
            'eligible_seconds': sum(m['eligible_seconds'] for m in measures),
            'observed_seconds': observed,
            'pair_mean': sum(m['mean'] for m in measures if m['mean'] is not None) /
                         max(1, sum(m['mean'] is not None for m in measures)) if observed else None,
            'worst_pair': max((p for p in pairs if p['metrics'][key]['max'] is not None),
                              key=lambda p: p['metrics'][key]['max'], default={}).get('pair'),
            'max': max((m['max'] for m in measures if m['max'] is not None), default=None),
            'above_seconds': {t: sum(m['above_seconds'][t] for m in measures)
                              for t in measures[0]['above_seconds']}}
    return {'summary': summary, 'pairs': pairs}


def cue_events(annotations, cues):
    if annotations is None or cues is None:
        return {'status': 'unavailable', 'reason': 'Independent annotations and cue export required'}
    offsets = []
    for event in annotations:
        candidates = cues.get(str(event['track_id']), [])
        offset = min((abs(t - event['time_sec']) for t in candidates), default=None)
        offsets.append({'track_id': event['track_id'], 'offset_seconds': offset,
                        'hit': offset is not None and offset <= event['tolerance_sec']})
    return {'status': 'observed', 'events': offsets,
            'recall': sum(e['hit'] for e in offsets) / len(offsets) if offsets else None}
