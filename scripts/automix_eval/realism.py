"""Transition score and comparison with published DJ practice.

Derived from observer measurements and the plan's own controls, never from
planner scores:

* `score`: 0-100 per transition and per run. Every check earns points for the
  tightest level it passes, and a transition with no deviation earns a further
  share, so clean transitions are rewarded and flawed ones keep partial credit.
* `clean_rate`: the share of observed transitions in which no check deviates.
* `dj_reference`: tempo and key manipulation per played track, next to the
  shares measured in real DJ mixes.

A playlist can hold anything in any order. Tempo is judged on the change beyond
what beatmatching the pair requires, and clashing native keys allow one more
semitone. That allowance ends with the overlap: `settle` judges the tempo and
key a track keeps afterwards. Every limit and point value is an engineering
assumption except the reference shares. None of this is listening acceptance.
"""
import math
from .signal import Plan

VERSION = 3

# Kim, Choi, Sacks, Yang, Nam: "A Computational Analysis of Real-World DJ Mixes
# using Mix-To-Track Subsequence Alignment", ISMIR 2020 (arXiv:2008.10267),
# sections 6.1 and 6.2. Their transition lengths peak at every 32 beats but the
# paper gives no shares, so overlap length stays a descriptor here.
REFERENCE = {
    'source': 'arXiv:2008.10267 (ISMIR 2020), sections 6.1 and 6.2',
    'population': '1,557 mixes from 1001Tracklists, 24,202 played tracks, mostly House and Trance',
    'tempo_within': {'0.05': .861, '0.1': .945, '0.2': .986},
    'transposed': .025,
    'transposed_one_semitone': .943,
}

LEVELS = {
    'strict': {'tempo': .05, 'semitones': 0, 'kept_tempo': .02, 'phase_ms': '20', 'roughness': '0.25',
               'voice_db': '3', 'phrase_loss': 0., 'crossing': True, 'abrupt_cut': True, 'play_out': False,
               'energy_db': '3', 'gap_seconds': .5},
    'default': {'tempo': .1, 'semitones': 1, 'kept_tempo': .05, 'phase_ms': '30', 'roughness': '0.4',
                'voice_db': '6', 'phrase_loss': 0., 'crossing': False, 'abrupt_cut': False, 'play_out': False,
                'energy_db': '6', 'gap_seconds': 1.},
    'lenient': {'tempo': .2, 'semitones': 2, 'kept_tempo': .1, 'phase_ms': '50', 'roughness': '0.55',
                'voice_db': '9', 'phrase_loss': .25, 'crossing': False, 'abrupt_cut': False, 'play_out': True,
                'energy_db': '9', 'gap_seconds': 2.},
}
# Seconds above a threshold tolerated before a check deviates, at every level.
LIMITS = {'beat_sync': 1., 'harmony': 2., 'voice': .5, 'energy': 1.}
CHECKS = ('structure', 'tempo', 'key', 'settle', 'exit', 'beat_sync', 'harmony', 'vocals', 'energy')
# Points for the tightest level a check passes, and the share of the score
# reserved for transitions without any deviation (half of it at lenient).
CREDIT = {'strict': 1., 'default': .8, 'lenient': .5}
CLEAN_SHARE = .4
# Heard overlap under this many seconds at the end of a record is a play-out.
PLAYED_OUT_SECONDS = 6.
# Rate and pitch below these are numerical noise, not a performed change.
RATE_STEP, PITCH_STEP, STEP = .002, .05, .25


def heard_states(plan, audible):
    """(seconds, rate, pitch) for each deck while it is heard in the plan."""
    states = ([], [])
    steps = max(1, math.ceil(plan.duration / STEP))
    dt = plan.duration / steps
    for i in range(steps):
        _, decks = plan.at((i + .5) * dt)
        for side, deck in enumerate(decks):
            if not deck['playing']:
                continue
            # Loops misplace the source; their controls still say what is open.
            if deck['heard'] is not None and not plan.unsupported:
                on = sum(map(sum, deck['heard'])) > audible
            else:
                on = max(map(max, deck['gains'])) > audible
            if on:
                states[side].append((dt, deck['rate'], deck['pitch']))
    return states


def camelot(value):
    try:
        number, mode = int(value[:-1]), value[-1].upper()
    except (TypeError, ValueError, IndexError):
        return None
    return (number % 12, mode) if mode in ('A', 'B') and 1 <= number <= 12 else None


def tempo_gap(a, b):
    """Signed log ratio from a's native tempo to b's, half and double folded."""
    bpms = [t.data['tempo'].get('global_bpm') for t in (a, b)]
    if not all(isinstance(v, (int, float)) and math.isfinite(v) and v > 0 for v in bpms):
        return 0.
    return min((math.log(bpms[1] * k / bpms[0]) for k in (.5, 1, 2)), key=abs)


def demand(a, b):
    """What the queue asks of this pair before any planner choice.

    Meeting in the middle is the smallest per-deck tempo change that beatmatches
    the native tempos, half and double time included. Native keys clash when
    they are neither equal, relative nor adjacent on the Camelot wheel; an
    unknown key claims no allowance. Labels under 0.55 confidence still count
    and are reported as weak, since a wrong label can grant the allowance.
    """
    forced = math.exp(abs(tempo_gap(a, b)) / 2) - 1
    ka, kb = camelot(a.data.get('camelot')), camelot(b.data.get('camelot'))
    clash = None
    if ka and kb:
        adjacent = ka[1] == kb[1] and (ka[0] - kb[0]) % 12 in (1, 11)
        clash = not (ka[0] == kb[0] or adjacent)
    weak = any((t.data.get('key_confidence') or 0) < .55 for t in (a, b))
    return forced, clash, weak


def kept(offset, track, following):
    """Tempo and key a track keeps once the plan, solo recovery included, ends.

    Only an offset towards the meeting point with the next track is useful
    there; the current pair no longer justifies anything.
    """
    useful = tempo_gap(track, following) / 2 if following else 0.
    rate = math.log(max(.01, offset['rate']))
    nearest = min(max(rate, min(0., useful)), max(0., useful))
    return math.exp(abs(rate - nearest)) - 1, abs(offset['pitch_semitones'])


def played_out(pair):
    """The outgoing record ran to its tail and was replaced almost unmixed.

    A safe handoff, but not a mix: the listener hears one record end and
    another begin. The nominal exit stands in when no sound was observed at
    a cut; a plan the observer cannot follow stays unknown.
    """
    handoff = pair['handoff']
    if handoff['status'] == 'unavailable':
        return None
    tail = handoff['heard_tail_position']
    if tail is None:
        tail = handoff['legacy_tail_short']
    if tail is None:
        return None
    return bool(tail) and handoff['overlap']['seconds_lower'] < PLAYED_OUT_SECONDS


def exposure(metric, threshold, limit):
    """False above the limit, None when unknown time could hide a deviation."""
    if metric['above_seconds'][threshold] > limit + 1e-8:
        return False
    if metric['eligible_seconds'] - metric['observed_seconds'] > limit + 1e-8:
        return None
    return True


def introduced(states, offset):
    """Largest tempo and pitch change this transition makes audible.

    The incoming deck answers for everything heard and for the state it keeps.
    The outgoing deck answers only when its carried state moves, so one choice
    is charged to the transition that made it.
    """
    out, incoming = states
    rate = max([abs(r - 1) for _, r, _ in incoming] + [abs(offset['rate'] - 1)])
    pitch = max([abs(p) for _, _, p in incoming] + [abs(offset['pitch_semitones'])])
    if out:
        if max(r for _, r, _ in out) - min(r for _, r, _ in out) > RATE_STEP:
            rate = max([rate] + [abs(r - 1) for _, r, _ in out])
        if max(p for _, _, p in out) - min(p for _, _, p in out) > PITCH_STEP:
            pitch = max([pitch] + [abs(p) for _, _, p in out])
    return rate, pitch


def checks(pair, level, tempo, pitch, settled, ended):
    metrics, integrity = pair['metrics'], pair['integrity']
    known = integrity['status'] != 'unavailable'
    partial = integrity['status'] == 'partial'
    result = {'structure': not pair['violations'],
              'tempo': tempo <= level['tempo'] + 1e-6,
              'key': pitch <= level['semitones'] + PITCH_STEP,
              'settle': (settled[0] <= level['kept_tempo'] + 1e-6
                         and settled[1] <= level['semitones'] + PITCH_STEP),
              'exit': None if ended is None else level['play_out'] or not ended,
              'beat_sync': exposure(metrics['grid_phase_ms'], level['phase_ms'], LIMITS['beat_sync']),
              'harmony': exposure(metrics['tonal_roughness'], level['roughness'], LIMITS['harmony'])}
    voice = exposure(metrics['voice_attenuation_db'], level['voice_db'], LIMITS['voice'])
    crossing = [pair['voice_exit_crossing'], pair['voice_entry_crossing']]
    lost = integrity['vocal_phrase_damage'] if known else None
    if voice is False or (known and lost > level['phrase_loss'] + 1e-8) \
            or (level['crossing'] and any(c is True for c in crossing)):
        result['vocals'] = False
    else:
        result['vocals'] = None if voice is None or not known or partial else True
    energy = exposure(metrics['peak_deficit_db'], level['energy_db'], LIMITS['energy'])
    # A cut replaces the outgoing tonal parts at once by definition. Only the
    # strict level holds that against it; the deficit and gap screens still apply.
    abrupt = known and integrity['abrupt_tonal_loss'] > .5 and (level['abrupt_cut'] or not pair['cut'])
    if energy is False or abrupt or (known and integrity['foreground_gap_seconds'] > level['gap_seconds'] + 1e-8):
        result['energy'] = False
    else:
        result['energy'] = None if energy is None or not known or partial else True
    return result


def credit(values, check):
    """Points for the tightest level passed; None when it was not observed."""
    for name, points in CREDIT.items():
        if values[name][check] is True:
            return points
    return 0. if values['lenient'][check] is False else None


def score(levels, credits):
    """0-100, or None when missing evidence could still hide a deviation."""
    if levels['lenient']['status'] == 'unknown':
        return None
    known = [c for c in credits.values() if c is not None]
    clean = (1. if levels['default']['status'] == 'clean' else
             .5 if levels['lenient']['status'] == 'clean' else 0.)
    return 100 * ((1 - CLEAN_SHARE) * sum(known) / len(known) + CLEAN_SHARE * clean)


def tail(k, n, p):
    """P(at least k of n) under the reference share; small n stays visible."""
    if k <= 0:
        return 1.
    return sum(math.comb(n, i) * p ** i * (1 - p) ** (n - i) for i in range(k, n + 1))


def played_tracks(plans, tracks, count, states):
    """Time-weighted tempo adjustment and transposition per played track."""
    played = []
    for j in range(count + 1):
        before, after = (plans[j - 1] if j else None), (plans[j] if j < len(plans) else None)
        track = tracks[before['summary']['pair'][1] if before else after['summary']['pair'][0]]
        offset = before['incoming_offset_end'] if before else after['outgoing_offset']
        start = before['t_end_b'] if before else 0.
        end = after['t_in_a'] if after else track.duration
        samples = [(max(0., end - start) / max(.01, offset['rate']),
                    offset['rate'], offset['pitch_semitones'])]
        if before:
            samples += states[j - 1][1]
        if j < count:
            samples += states[j][0]
        seconds = sum(s for s, _, _ in samples)
        if seconds <= 0:
            continue
        played.append({'track_id': track.id, 'heard_seconds': seconds,
                       'tempo_adjustment': sum(s * abs(r - 1) for s, r, _ in samples) / seconds,
                       'peak_tempo_adjustment': max(abs(r - 1) for s, r, _ in samples if s > 0),
                       'transposition_semitones': max(abs(p) for s, _, p in samples if s > 0)})
    return played


def reference(played):
    n = len(played)
    if not n:
        return {'status': 'not_applicable', 'played_tracks': 0}
    result = {'status': 'observed', 'played_tracks': n, 'reference': REFERENCE, 'tempo_within': {}}
    gaps = []
    for limit, share in REFERENCE['tempo_within'].items():
        outside = sum(t['tempo_adjustment'] >= float(limit) for t in played)
        result['tempo_within'][limit] = {
            'share': 1 - outside / n, 'reference_share': share, 'tracks_outside': outside,
            'chance_under_reference': tail(outside, n, 1 - share)}
        gaps.append(abs(1 - outside / n - share))
    shifted = [t['transposition_semitones'] for t in played if t['transposition_semitones'] >= .5]
    result['transposed'] = {
        'share': len(shifted) / n, 'reference_share': REFERENCE['transposed'],
        'tracks': len(shifted), 'chance_under_reference': tail(len(shifted), n, REFERENCE['transposed']),
        'one_semitone_share': sum(s < 1.5 for s in shifted) / len(shifted) if shifted else None,
        'reference_one_semitone_share': REFERENCE['transposed_one_semitone'],
        'max_semitones': max(shifted, default=0.)}
    gaps.append(abs(len(shifted) / n - REFERENCE['transposed']))
    # One distance for trend tracking: 0 matches the reference shares exactly.
    result['mean_gap_percentage_points'] = 100 * sum(gaps) / len(gaps)
    return result


def overlap(pairs):
    """Descriptors only: the reference gives no shares for overlap length."""
    known = [p for p in pairs if p['handoff']['status'] != 'unavailable']
    bars = sorted(p['handoff']['overlap']['bars_lower'] for p in known)
    nominal = [p['reported_bars'] for p in pairs]
    return {'observed_pairs': len(known),
            'heard_blend_pairs': sum(b >= 4 for b in bars),
            'nominal_blend_pairs': sum(b >= 4 for b in nominal),
            'median_heard_bars': bars[len(bars) // 2] if bars else None,
            'heard_bars': sum(bars), 'nominal_bars': sum(nominal),
            'nominal_eight_bar_multiples': sum(b > 0 and b % 8 == 0 for b in nominal),
            'cuts': sum(p['cut'] for p in pairs),
            'played_out_pairs': sum(p['realism']['played_out'] is True for p in pairs)}


def annotate(result, plans, tracks, audible=.02):
    pairs = result['pairs']
    states = [heard_states(Plan(plan, tracks), audible) for plan in plans[:len(pairs)]]
    summary = {name: {'clean': 0, 'deviating': 0, 'unknown': 0,
                      'checks': {c: {'clean': 0, 'deviating': 0, 'unknown': 0} for c in CHECKS}}
               for name in LEVELS}
    for index, (pair, plan, state) in enumerate(zip(pairs, plans, states)):
        tempo, pitch = introduced(state, plan['incoming_offset_end'])
        forced, clash, weak = demand(*(tracks[i] for i in plan['summary']['pair']))
        excess, allowance = max(0., tempo - forced), 1 if clash else 0
        following = tracks[plans[index + 1]['summary']['pair'][1]] if index + 1 < len(plans) else None
        settled = kept(plan['incoming_offset_end'], tracks[plan['summary']['pair'][1]], following)
        ended = played_out(pair)
        values = {name: checks(pair, level, excess, max(0., pitch - allowance), settled, ended)
                  for name, level in LEVELS.items()}
        levels = {}
        for name in LEVELS:
            deviating = [c for c in CHECKS if values[name][c] is False]
            unknown = [c for c in CHECKS if values[name][c] is None]
            status = 'deviating' if deviating else 'unknown' if unknown else 'clean'
            levels[name] = {'status': status, 'deviating': deviating, 'unknown': unknown}
            summary[name][status] += 1
            for c in CHECKS:
                key = ('deviating' if values[name][c] is False else
                       'unknown' if values[name][c] is None else 'clean')
                summary[name]['checks'][c][key] += 1
        credits = {c: credit(values, c) for c in CHECKS}
        pair['realism'] = {'score': score(levels, credits), 'credits': credits,
                           'played_out': ended,
                           'tempo_adjustment': tempo, 'forced_tempo_adjustment': forced,
                           'tempo_excess': excess, 'transposition_semitones': pitch,
                           'kept_tempo_excess': settled[0], 'kept_semitones': settled[1],
                           'native_key_clash': clash, 'weak_key_labels': weak, 'levels': levels}
    for level in summary.values():
        judged = level['clean'] + level['deviating']
        # Unknown transitions bound the rate; they never count as clean.
        level['clean_rate'] = level['clean'] / judged if judged else None
        level['clean_rate_bounds'] = ([level['clean'] / len(pairs),
                                       (level['clean'] + level['unknown']) / len(pairs)]
                                      if pairs else None)
    scored = [p['realism']['score'] for p in pairs if p['realism']['score'] is not None]
    unscored = len(pairs) - len(scored)
    demands = [p['realism'] for p in pairs]
    played = played_tracks(plans, tracks, len(pairs), states) if pairs else []
    result['realism'] = {'version': VERSION, 'interpretation': 'proxy_score_not_listening_acceptance',
                         'score': sum(scored) / len(scored) if scored else None,
                         # Unscored transitions bound the run between 0 and 100 each.
                         'score_bounds': ([sum(scored) / len(pairs), (sum(scored) + 100 * unscored) / len(pairs)]
                                          if pairs else None),
                         'scored_transitions': len(scored), 'unscored_transitions': unscored,
                         'points': {'credit': CREDIT, 'clean_share': CLEAN_SHARE},
                         'queue': {'forced_tempo_over_5_percent_pairs': sum(d['forced_tempo_adjustment'] >= .05 for d in demands),
                                   'native_key_clash_pairs': sum(d['native_key_clash'] is True for d in demands),
                                   'unknown_key_pairs': sum(d['native_key_clash'] is None for d in demands),
                                   'weak_key_label_pairs': sum(d['weak_key_labels'] for d in demands)},
                         'levels': summary, 'thresholds': LEVELS, 'tolerated_seconds': LIMITS,
                         'dj_reference': reference(played), 'played_tracks': played,
                         'overlap': overlap(pairs)}
    return result
