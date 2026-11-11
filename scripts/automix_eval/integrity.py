"""Audible continuity diagnostics, independent of planner scores and gates.

These are conservative signal proxies, not a learned listening-quality model.
The source phrase continues through short inter-word rests, even after its lane
has become silent. Missing evidence is unknown, not a perfect score.
"""
from bisect import bisect_right
import math
from .signal import Plan
from .instrument_notes import observe as observe_notes


def pulse(track):
    bpm = track.data['tempo'].get('global_bpm', 120)
    if not math.isfinite(bpm) or bpm <= 0:
        bpm = 120
    while bpm > 160:
        bpm /= 2
    while 0 < bpm < 80:
        bpm *= 2
    return 60 / max(1, bpm)


def vocal_phrases(track, gap_scale=1):
    key = ('integrity_phrases', gap_scale)
    cache = getattr(track, '_integrity_cache', {})
    if key in cache:
        return cache[key]
    phrases = []
    gap = 2 * pulse(track) * gap_scale
    for f, power in zip(track.frames, track.powers):
        # Relative band power makes gain changes irrelevant. A quiet sustained
        # note still counts when the detector has explicit note evidence.
        at = (f['start_sec'] + f['end_sec']) / 2
        held = bisect_right(track.note_starts, at) > bisect_right(track.note_ends, at)
        if sum(power[0]) < .002 or (f['vocal_activity'] < .2 and not held):
            continue
        if phrases and f['start_sec'] - phrases[-1][1] <= gap:
            phrases[-1][1] = f['end_sec']
        else:
            phrases.append([f['start_sec'], f['end_sec']])
    cache[key] = phrases
    track._integrity_cache = cache
    return phrases


def source_wall(plan, side, source):
    return plan.source_wall(side, source)


def sustained_instruments(track):
    """Held-note evidence, not inferred melodic sentence boundaries."""
    notes = sorted((n['start_sec'], n['end_sec'])
                   for n in (track.data.get('stems') or {}).get('notes', [])
                   if n['stem'] == 'instruments' and n['confidence'] >= .5
                   and n['end_sec'] - n['start_sec'] >= pulse(track))
    spans = []
    for start, end in notes:
        if spans and start < spans[-1][1]:
            spans[-1][1] = max(end, spans[-1][1])
        else:
            spans.append([start, end])
    return spans


def phrase_events(plan, track, side, spans, parts, previous=None):
    data = plan.data
    source_start = data['t_in_b' if side else 't_in_a']
    source_end = data['t_end_b' if side else 't_out_a']
    entered = previous.data['t_in_b'] if not side and previous else 0
    events = []
    unknown = 0.
    for start, end in spans:
        if end <= (source_start if side else max(entered, source_start - pulse(track))) or start >= source_end + pulse(track):
            continue
        energy = lost = exposed = lost_seconds = 0.
        for frame, bands in zip(track.frames, track.powers):
            lo, hi = max(start, frame['start_sec']), min(end, frame['end_sec'])
            if not side:
                lo = max(lo, entered)
            if hi <= lo:
                continue
            source = (lo + hi) / 2
            context, context_side = plan, side
            if not side and previous and source < previous.data['t_end_b']:
                context, context_side = previous, 1
            wall = context.source_wall(context_side, source)
            bounds = [context.source_wall(context_side, t) for t in (lo, hi)]
            if wall is None or any(t is None for t in bounds):
                unknown += hi - lo
                continue
            dt = max(0., bounds[1] - bounds[0])
            _, decks = context.at(wall)
            deck = decks[context_side]
            raw = sum(sum(bands[part]) for part in parts)
            if raw < .002:
                continue
            gain = sum(p * g for part in parts for p, g in zip(bands[part], deck['gains'][part])) / raw
            energy += raw * dt
            lost += raw * dt * max(0, 1 - gain)
            exposed += dt * (gain > .5)
            lost_seconds += dt * (gain < .5)
        if exposed >= .15 and energy > 0 and lost / energy > .1 and lost_seconds >= .15:
            events.append({'side': side, 'source_start': start, 'source_end': end,
                           'lost_energy_fraction': lost / energy,
                           'lost_voiced_seconds' if parts == (0,) else 'lost_note_seconds': lost_seconds})
    return events, unknown


def evaluate(data, tracks, previous=None, gap_scale=1):
    plan = Plan(data, tracks)
    reason = plan.unsupported or (['missing_stems'] if not plan.a.frames or not plan.b.frames else [])
    if reason:
        return {'status': 'unavailable', 'reason': reason, 'score': None,
                'eligible_seconds': plan.duration + .2, 'observed_seconds': 0.}
    previous_plan = Plan(previous, tracks) if previous else None
    events, melody_events, note_events, unknown_phrases, unknown_notes = [], [], [], 0., 0
    for side, track in enumerate((plan.a, plan.b)):
        found, unknown = phrase_events(plan, track, side, vocal_phrases(track, gap_scale), (0,), previous_plan)
        events.extend(found)
        unknown_phrases += unknown
        found, unknown = phrase_events(plan, track, side, sustained_instruments(track), (2, 3), previous_plan)
        melody_events.extend(found)
        unknown_phrases += unknown
        found, unknown = observe_notes(plan, track, side, pulse(track), previous_plan)
        note_events.extend(found)
        unknown_notes += unknown
    gap_seconds = foreground_gap = voice_carry = abrupt = observed = 0.
    old = None
    duration = plan.duration + .2
    for i in range(math.ceil(duration / .05)):
        dt = min(.05, duration - i * .05)
        wall = -.1 + i * .05 + dt / 2
        _, decks = plan.at(wall)
        if any(d['raw'] is None for d in decks if d['playing']):
            old = None
            continue
        observed += dt
        raw = [sum(d['raw']['power'][2][1:]) if d['raw'] else 0 for d in decks]
        heard = [sum(d['heard'][2][1:]) if d['heard'] else 0 for d in decks]
        voice = sum(sum(d['heard'][0]) for d in decks if d['heard'])
        available = max((v for v, d in zip(raw, decks) if d['playing']), default=0)
        if 0 <= wall < plan.mix_end:
            if available > .02 and sum(heard) < available * .125:
                gap_seconds += dt
                if voice > .02:
                    voice_carry += dt
            # Measure removal of A's natural foreground. A louder, still-muted
            # B does not establish a foreground that the listener has lost.
            # Incoming phrase damage is observed separately by phrase_events.
            foreground = raw[0] + (sum(decks[0]['raw']['power'][0])
                                   if decks[0]['raw'] else 0)
            if foreground > .02 and voice <= .02 and sum(heard) + voice < foreground * .125:
                foreground_gap += dt
        if old:
            for j in range(2):
                old_raw, old_heard = old[j]
                if min(raw[j], old_raw) > .02 and old_heard > .02:
                    if heard[j] / raw[j] < old_heard / old_raw * .25:
                        abrupt = max(abrupt, min(1, old_heard / max(.02, sum(v[1] for v in old))))
        old = list(zip(raw, heard))
    return {'status': 'observed' if duration - observed < 1e-6 and not unknown_phrases else 'partial',
            'score': None, 'score_status': 'retired_uncalibrated',
            'eligible_seconds': duration, 'observed_seconds': observed,
            'unknown_phrase_source_seconds': unknown_phrases,
            'vocal_phrase_damage': max((e['lost_energy_fraction'] for e in events), default=0),
            'vocal_events': events, 'sustained_instrument_events': melody_events,
            'instrument_note_events': note_events,
            'unknown_instrument_notes': unknown_notes,
            'instrument_interpretation': 'control_observations_not_phrase_damage',
            'instrument_bed_gap_seconds': round(gap_seconds, 3),
            'foreground_gap_seconds': round(foreground_gap, 3),
            'voice_carried_bed_gap_seconds': round(voice_carry, 3),
            'abrupt_tonal_loss': abrupt}


def summarize(pairs):
    all_values = [p['integrity'] for p in pairs]
    values = [v for v in all_values if v['status'] != 'unavailable']
    return {'observed_pairs': sum(v['status'] == 'observed' for v in values),
            'partial_pairs': sum(v['status'] == 'partial' for v in values),
            'unavailable_pairs': len(pairs) - len(values),
            'eligible_seconds': sum(v['eligible_seconds'] for v in all_values),
            'observed_seconds': sum(v['observed_seconds'] for v in all_values),
            'unknown_phrase_source_seconds': sum(v['unknown_phrase_source_seconds'] for v in values),
            'vocal_damage_pairs': sum(bool(v['vocal_events']) for v in values),
            'sustained_instrument_risk_pairs': sum(bool(v['sustained_instrument_events']) for v in values),
            'instrument_bed_gap_seconds': sum(v['instrument_bed_gap_seconds'] for v in values),
            'foreground_gap_seconds': sum(v['foreground_gap_seconds'] for v in values),
            'voice_carried_bed_gap_seconds': sum(v['voice_carried_bed_gap_seconds'] for v in values),
            'abrupt_tonal_loss_pairs': sum(v['abrupt_tonal_loss'] > .5 for v in values)}
