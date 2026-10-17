"""Read-only signal/control observer. No planner scores or production decisions."""
from bisect import bisect_right
import math


def quantile(values, q):
    values = sorted(values)
    return values[min(len(values) - 1, int(q * (len(values) - 1)))] if values else None


def power(db):
    return 0.0 if db <= -90 else 10 ** (db / 10)


class Curve:
    def __init__(self, value, default=0, logarithmic=False):
        nodes = value.get('nodes', []) if value else []
        self.x = [p[0] for p in nodes]
        self.y = [p[1] for p in nodes]
        if any(b < a for a, b in zip(self.x, self.x[1:])):
            raise ValueError('Non-monotonic curve coordinates')
        if not all(math.isfinite(v) for p in nodes for v in p):
            raise ValueError('Non-finite curve')
        self.default, self.logarithmic = default, logarithmic

    def at(self, x):
        if not self.x:
            return self.default
        i = bisect_right(self.x, x)
        if i == 0:
            return self.y[0]
        if i == len(self.x):
            return self.y[-1]
        t = (x - self.x[i - 1]) / (self.x[i] - self.x[i - 1])
        a, b = self.y[i - 1:i + 1]
        if self.logarithmic:
            return math.exp(math.log(max(1, a)) * (1 - t) + math.log(max(1, b)) * t)
        return a + t * (b - a)

    def inverse(self, value):
        if any(b <= a for a, b in zip(self.y, self.y[1:])):
            raise ValueError('Clock must be strictly increasing')
        result = Curve(None)
        result.x, result.y = self.y, self.x
        return result.at(value)


class Track:
    def __init__(self, data):
        self.data = data
        self.id = data['track_id']
        self.duration = data['duration_sec']
        self.frames = (data.get('stems') or {}).get('frames', [])
        self.starts = [f['start_sec'] for f in self.frames]
        self.beats = data['tempo']['beats']
        self.confidence = data['tempo'].get('pulse_confidence', [])
        self.segments = data['tempo'].get('segments', [])
        self.segment_starts = [s['start_beat'] for s in self.segments]
        all_notes = (data.get('stems') or {}).get('notes', [])
        self.notes = [n for n in all_notes if n['stem'] == 'vocals' and n['confidence'] >= .5]
        self.note_starts = sorted(n['start_sec'] for n in self.notes)
        self.note_ends = sorted(n['end_sec'] for n in self.notes)
        self.chroma = [[[0.0] * 12 for _ in range(2)] for _ in self.frames]
        for note in all_notes:
            if note['stem'] not in ('vocals', 'instruments') or 'midi' not in note:
                continue
            group = 0 if note['stem'] == 'vocals' else 1
            lo = max(0, bisect_right(self.starts, note['start_sec']) - 1)
            hi = bisect_right(self.starts, note['end_sec'])
            for i in range(lo, hi):
                f = self.frames[i]
                covered = max(0, min(note['end_sec'], f['end_sec']) - max(note['start_sec'], f['start_sec']))
                self.chroma[i][group][note['midi'] % 12] += note['confidence'] * covered
        totals = [sum(power(db) for band in f['band_db'] for db in band) for f in self.frames]
        active = [v for v in totals if v > max(totals, default=0) * .001]
        self.reference = max(quantile(active, .75) or 0, 1e-12)
        self.powers = []
        for f in self.frames:
            # The analysis groups bass with instruments; only the low band is
            # available as an estimate of its contribution to the fourth lane.
            p = [[0.0] * 3 for _ in range(4)]
            for stem in range(3):
                for band in range(3):
                    part = 3 if stem == 2 and band == 0 else stem
                    p[part][band] = power(f['band_db'][stem][band]) / self.reference
            self.powers.append(p)
        self.onsets = [0.0]
        for f in self.frames:
            self.onsets.append(self.onsets[-1] + max(0, f['onset'][2]))

    def at(self, sec):
        i = bisect_right(self.starts, sec) - 1
        if i < 0 or sec >= self.frames[i]['end_sec'] or sec >= self.duration:
            return None
        f = self.frames[i]
        held = bisect_right(self.note_starts, sec) > bisect_right(self.note_ends, sec)
        lo = bisect_right(self.starts, sec - .5)
        hi = min(len(self.frames), bisect_right(self.starts, sec + .5))
        rhythmic_instruments = self.onsets[hi] - self.onsets[lo] > 1e-6
        return {'power': self.powers[i], 'chroma': self.chroma[i],
                'voice': f['vocal_activity'] >= .35 or held,
                'drum_onset': f['onset'][1],
                'rhythmic_instruments': rhythmic_instruments}

    def grid(self, sec, rate):
        i = bisect_right(self.beats, sec) - 1
        if i < 0 or i + 1 >= len(self.beats) or rate <= 0:
            return None
        beat = i + (sec - self.beats[i]) / max(1e-8, self.beats[i + 1] - self.beats[i])
        segment = bisect_right(self.segment_starts, beat) - 1
        if segment < 0 or beat >= self.segments[segment]['end_beat']:
            return None
        # Protocol confidence is per tempo segment (usually sixteen beats),
        # not per beat. Missing pulse evidence is unknown in this observer.
        if segment >= len(self.confidence) or self.confidence[segment] < .15:
            return None
        period = (self.beats[i + 1] - self.beats[i]) / rate
        return period, (sec - self.beats[i]) / rate


def shift_chroma(chroma, semitones):
    out = [0.0] * 12
    for i, value in enumerate(chroma):
        x = i + semitones
        lo = math.floor(x)
        out[lo % 12] += value * (1 - (x - lo))
        out[(lo + 1) % 12] += value * (x - lo)
    total = sum(out)
    return [v / total for v in out] if total > 1e-8 else None


def roughness(a, b):
    # Interval-class proxy, not psychoacoustic sensory dissonance: semitone,
    # whole tone and tritone are less stable than thirds/fourths/fifths.
    weights = (0, 1, .7, .25, .2, .15, .9, .15, .2, .25, .7, 1)
    return sum(x * y * weights[(j - i) % 12]
               for i, x in enumerate(a) for j, y in enumerate(b))


def heard_chroma(deck):
    """Do not let a muted vocal's notes certify the remaining instruments."""
    if not deck['raw'] or not deck['heard']:
        return None
    weights = (sum(deck['heard'][0]), sum(deck['heard'][2]) + sum(deck['heard'][3]))
    mixed = [0.0] * 12
    known = 0.0
    for notes, weight in zip(deck['raw']['chroma'], weights):
        norm = sum(notes)
        if norm > 1e-8:
            known += weight
            for i, value in enumerate(notes):
                mixed[i] += weight * value / norm
    # Partial note detection is not evidence about all audible tonal material.
    if known < .8 * sum(weights) or known <= 1e-8:
        return None
    return shift_chroma(mixed, deck['pitch'])


def grid_error(a, b):
    pa, xa = a
    pb, xb = b
    ratio = min((.5, 1, 2), key=lambda k: abs(math.log(pa / (pb * k))))
    mismatch = abs(pa / (pb * ratio) - 1)
    # Compare on the faster equivalent pulse lattice. This does not establish
    # downbeat parity when a recording admits a half/double interpretation.
    period = min(pa, pb)
    phase = abs((xa - xb + period / 2) % period - period / 2)
    return mismatch, phase * 1000


class Plan:
    def __init__(self, data, tracks):
        self.data = data
        self.a, self.b = [tracks[i] for i in data['summary']['pair']]
        self.clock = Curve(data['clock'])
        if any(b <= a for a, b in zip(self.clock.y, self.clock.y[1:])):
            raise ValueError('Clock must be strictly increasing')
        self.duration = self.clock.y[-1] if self.clock.y else 0
        self.end_bar = self.clock.x[-1] if self.clock.x else 0
        self.inverse = Curve(None)
        self.inverse.x, self.inverse.y = self.clock.y, self.clock.x
        self.xfader = Curve(data['lanes']['xfader'])
        self.sources = [Curve(data[k]) for k in ('outgoing_source', 'incoming_source')]
        self.mix_end = self.clock.at(self.sources[0].x[-1]) if self.sources[0].x else 0
        self.curves = []
        for side, name in zip(('a', 'b'), ('outgoing', 'incoming')):
            l = data['lanes']
            self.curves.append({
                'gain': Curve(l['gain_' + side]),
                'rate': Curve(l['rate_' + side], 1),
                'pitch': Curve(l['pitch_' + side]),
                'eq': [Curve(l['eq_' + side][band]) for band in ('low', 'mid', 'high')],
                'lp': Curve(l['filter_' + side]['lp_hz'], 20000, True),
                'hp': Curve(l['filter_' + side]['hp_hz'], 20, True),
                'stems': [Curve(v, 1) for v in data['stem_mix'][name]]
                if data.get('stem_mix') else [Curve(None, 1) for _ in range(4)]})
        self.unsupported = [k for k in ('loop_a', 'loop_b', 'scratch_a', 'scratch_b')
                            if data['lanes'].get(k)]
        if data.get('insert_a'):
            self.unsupported.append('insert_a')

    def at(self, wall):
        u = self.inverse.at(wall)
        x = max(0, min(1, (self.xfader.at(u) + 1) / 2))
        result = []
        for side, track in enumerate((self.a, self.b)):
            c = self.curves[side]
            sec = self.sources[side].at(u)
            playing = not side or u >= self.data.get('incoming_start_bar', 0)
            if side == 0 and wall >= self.mix_end:
                playing = False
                sec = self.data['t_out_a'] + (wall - self.mix_end) * c['rate'].at(u)
            fader_power = (x if side else 1 - x) if playing else 0
            # Context makes cuts measurable, and prevents silent preparation
            # from reading a stationary incoming source as audible material.
            if wall < 0:
                playing = side == 0
                sec = (self.data['t_in_a'] + wall * c['rate'].at(0)) if playing else sec
            elif wall >= self.duration:
                playing = side == 1
                end = self.data['t_end_b'] if side else self.data['t_out_a']
                sec = end + (wall - self.duration) * c['rate'].at(u)
            raw = track.at(sec)
            level = fader_power * power(c['gain'].at(u))
            lp, hp = max(1, c['lp'].at(u)), max(1, c['hp'].at(u))
            band_gains = [level * power(c['eq'][band].at(u)) /
                          ((1 + (hz / lp) ** 4) * (1 + (hp / hz) ** 4))
                          for band, hz in enumerate((70, 600, 5000))]
            gains = []
            for part in range(4):
                bands = []
                stem = max(0, min(1, c['stems'][part].at(u))) ** 2
                for gain in band_gains:
                    g = gain * stem
                    if wall < 0 or wall >= self.duration:
                        g = 1.0 if playing else 0.0
                    bands.append(g)
                gains.append(bands)
            heard = [[p * g for p, g in zip(r, gs)] for r, gs in zip(raw['power'], gains)] if raw else None
            result.append({'raw': raw, 'heard': heard, 'gains': gains, 'source': sec,
                           'pitch': c['pitch'].at(u), 'rate': c['rate'].at(u), 'playing': playing})
        return u, result
