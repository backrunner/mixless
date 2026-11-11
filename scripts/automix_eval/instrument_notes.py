"""Per-note control observations, not a melodic-phrase damage classifier.

Do not join overlapping pitches into a sentence. Record whether an individual
estimated attack is hidden or a release is attenuated, plus the control speed.
These remain review observations: note transcription does not separate a lead,
pad or reverberation, and gradual removal of a repeating layer can be intended.
"""
from bisect import bisect_right
import math


def observe(plan, track, side, pulse, previous=None):
    notes = [n for n in (track.data.get('stems') or {}).get('notes', [])
             if n['stem'] == 'instruments' and n['confidence'] >= .5
             and all(math.isfinite(n[k]) for k in ('start_sec', 'end_sec'))
             and n['end_sec'] > n['start_sec']]
    start = plan.data['t_in_b' if side else 't_in_a']
    end = plan.data['t_end_b' if side else 't_out_a']
    entered = previous.data['t_in_b'] if previous and not side else 0
    events, unknown = [], 0
    fractions = (.05, .25, .5, .75, .95)
    for note in notes:
        lo, hi = note['start_sec'], note['end_sec']
        if hi <= max(entered, start - pulse) or lo >= end + pulse:
            continue
        values, heard_shares = [], []
        for fraction in fractions:
            sec = lo + (hi - lo) * fraction
            context, deck = plan, side
            if previous and not side and sec < previous.data['t_end_b']:
                context, deck = previous, 1
            wall = context.source_wall(deck, sec)
            at = bisect_right(track.starts, sec) - 1
            if wall is None or at < 0 or track.frames[at]['end_sec'] <= sec:
                values = []
                break
            bands = track.powers[at]
            raw = sum(bands[2]) + sum(bands[3])
            if raw < .002:
                values = []
                break
            _, decks = context.at(wall)
            gains = decks[deck]['gains']
            # The analysis groups bass with instruments. This is measured
            # tonal-lane audibility, not isolated MIDI-note energy.
            values.append(sum(p*g for part in (2, 3)
                              for p, g in zip(bands[part], gains[part])) / raw)
            heard = sum(sum(d['heard'][part]) for d in decks if d['heard'] for part in range(4))
            heard_shares.append(raw * values[-1] / max(1e-12, heard))
        if not values:
            unknown += 1
            continue
        peak = max(values)
        if peak <= 1e-6 or max(heard_shares) < .01:
            continue  # An entirely unplayed note is not a truncated heard note.
        relative = [v/peak for v in values]
        hidden_attack = relative[0] < .25 and max(relative[1:]) >= .5
        attenuated_release = relative[-1] < .25 and max(relative[:-1]) >= .5
        if not hidden_attack and not attenuated_release:
            continue
        speed = max(abs(b-a) / max(1e-6, (hi-lo)*(y-x)/pulse)
                    for a,b,x,y in zip(relative, relative[1:], fractions, fractions[1:]))
        events.append({'side': side, 'source_start': lo, 'source_end': hi,
                       'midi': note.get('midi'), 'confidence': note['confidence'],
                       'hidden_attack': hidden_attack,
                       'attenuated_release': attenuated_release,
                       'duration_pulses': (hi-lo)/pulse,
                       'max_power_gain_change_per_pulse': speed,
                       'power_gains': values,
                       'relative_power_gains': relative,
                       'max_tonal_lane_mix_share': max(heard_shares),
                       'interpretation': 'review_not_phrase_damage'})
    return events, unknown
