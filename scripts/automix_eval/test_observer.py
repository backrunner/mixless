import copy
import math
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from automix_eval.metrics import cue_events, evaluate_pair, evaluate_set
from automix_eval.signal import Curve, Plan, Track, grid_error, heard_chroma, roughness, shift_chroma


def curve(*nodes):
    return {'nodes': list(nodes)}


def track(tid, pitch=0, voice=False, duration=30):
    frames = []
    for i in range(round(duration * 20)):
        frames.append({'start_sec': i / 20, 'end_sec': (i + 1) / 20,
                       'band_db': [[-20 if voice else -120, -10 if voice else -120, -30 if voice else -120],
                                   [-10, -20, -20], [-20, -10, -20]],
                       'note_chroma': [float(j == pitch) for j in range(12)],
                       'onset': [0, 1, .1], 'vocal_activity': 1 if voice else 0})
    return {'track_id': tid, 'duration_sec': duration, 'sample_rate': 48000,
            'tempo': {'beats': [i / 2 for i in range(duration * 2)],
                      'segments': [{'start_beat': 0, 'end_beat': duration * 2}],
                      'pulse_confidence': [1]},
            'stems': {'frames': frames, 'notes': [
                {'stem': stem, 'start_sec': 0, 'end_sec': duration, 'midi': pitch + 60, 'confidence': 1}
                for stem in (('vocals', 'instruments') if voice else ('instruments',))]}, 'sections': []}


def plan(duration=8):
    lanes = {'xfader': curve((0, 0), (4, 0)), 'loop_a': None, 'loop_b': None,
             'scratch_a': None, 'scratch_b': None}
    for s in ('a', 'b'):
        lanes.update({'gain_' + s: curve((0, 0)), 'rate_' + s: curve((0, 1)),
                      'pitch_' + s: curve((0, 0)),
                      'eq_' + s: {b: curve((0, 0)) for b in ('low', 'mid', 'high')},
                      'filter_' + s: {'lp_hz': curve((0, 20000)), 'hp_hz': curve((0, 20))}})
    return {'summary': {'pair': [1, 2], 'length_bars': 4 if duration else 0,
                        'strategy': 'stem_swap', 'used_fallback': False},
            'clock': curve((0, 0), (4, duration)) if duration else curve((0, 0)),
            'outgoing_source': curve((0, 4), (4, 4 + duration)),
            'incoming_source': curve((0, 4), (4, 4 + duration)),
            'outgoing_offset': {'rate': 1, 'pitch_semitones': 0},
            'incoming_offset_end': {'rate': 1, 'pitch_semitones': 0},
            't_in_a': 4, 't_out_a': 4 + duration, 't_in_b': 4, 't_end_b': 4 + duration,
            'incoming_start_bar': 0, 'lanes': lanes, 'stem_mix': None,
            'insert_a': None, 'send_fx': None}


class ObserverTests(unittest.TestCase):
    def setUp(self):
        self.data = [track(1), track(2)]
        self.tracks = {t['track_id']: Track(t) for t in self.data}

    def test_curve_logarithmic_filter_and_reject_invalid_clock(self):
        self.assertAlmostEqual(Curve(curve((0, 20), (2, 2000)), logarithmic=True).at(1), 200)
        p = plan()
        p['clock'] = curve((0, 0), (2, 5), (4, 4))
        with self.assertRaises(ValueError):
            Plan(p, self.tracks)

    def test_aligned_and_quarter_beat_offset(self):
        self.assertEqual(grid_error((.5, .2), (.5, .2)), (0, 0))
        self.assertAlmostEqual(grid_error((.5, .2), (.5, .325))[1], 125)
        self.assertEqual(grid_error((1, .7), (.5, .2)), (0, 0))

    def test_grid_confidence_is_per_segment_not_per_beat(self):
        self.assertIsNotNone(self.tracks[1].grid(20, 1))
        self.tracks[1].confidence = []
        self.assertIsNone(self.tracks[1].grid(20, 1))

    def test_phase_error_through_full_plan_observer(self):
        p = plan()
        aligned = evaluate_pair(p, self.tracks)['metrics']['grid_phase_ms']
        self.assertAlmostEqual(aligned['mean'], 0)
        p['incoming_source'] = curve((0, 4.125), (4, 12.125))
        p['t_in_b'], p['t_end_b'] = 4.125, 12.125
        shifted = evaluate_pair(p, self.tracks)['metrics']['grid_phase_ms']
        self.assertAlmostEqual(shifted['mean'], 125)
        self.assertAlmostEqual(shifted['above_seconds']['30'], 8)

    def test_drum_activity_jump_is_separate_from_aligned_grid(self):
        p = plan()
        base = evaluate_pair(p, self.tracks)
        dense = copy.deepcopy(self.data)
        for f in dense[0]['stems']['frames']:
            f['onset'][1] = .01
        changed = evaluate_pair(p, {t['track_id']: Track(t) for t in dense})
        self.assertAlmostEqual(changed['metrics']['grid_phase_ms']['mean'], 0)
        self.assertGreater(changed['metrics']['drum_activity_change']['max'],
                           base['metrics']['drum_activity_change']['max'])
        cut = evaluate_pair(plan(0), {t['track_id']: Track(t) for t in dense})
        self.assertGreater(cut['metrics']['drum_activity_change']['max'], .1)

    def test_pitch_shift_matching_and_fractional_pitch(self):
        a = shift_chroma([1] + [0] * 11, 0)
        b = shift_chroma([0, 1] + [0] * 10, -1)
        self.assertEqual(roughness(a, b), 0)
        self.assertGreater(roughness(a, shift_chroma(b, 1)), .9)
        self.assertAlmostEqual(sum(shift_chroma(a, -.25)), 1)

    def test_silent_preparation_is_not_audible_overlap(self):
        p = plan()
        before = evaluate_pair(p, self.tracks)['audible_overlap_seconds']
        p['incoming_start_bar'] = 3
        after = evaluate_pair(p, self.tracks)['audible_overlap_seconds']
        self.assertAlmostEqual(before, 8)
        self.assertAlmostEqual(after, 2)

    def test_missing_stems_and_weak_grid_are_not_perfect_scores(self):
        self.data[0]['stems'] = None
        t = {d['track_id']: Track(d) for d in self.data}
        r = evaluate_pair(plan(), t)
        self.assertIsNone(r['metrics']['grid_phase_ms']['mean'])
        self.assertLess(r['stem_observed_deck_seconds'], r['stem_eligible_deck_seconds'])
        self.tracks[1].confidence = [0] * len(self.tracks[1].beats)
        m = evaluate_pair(plan(), self.tracks)['metrics']['grid_phase_ms']
        self.assertGreater(m['eligible_seconds'], 0)
        self.assertEqual(m['observed_seconds'], 0)

    def test_gain_and_time_translation_invariance(self):
        original = evaluate_pair(plan(), self.tracks)
        modified = copy.deepcopy(self.data)
        for t in modified:
            for f in t['stems']['frames']:
                f['band_db'] = [[v + 8 for v in band] for band in f['band_db']]
        r = evaluate_pair(plan(), {t['track_id']: Track(t) for t in modified})
        self.assertAlmostEqual(original['metrics']['foreground_competition']['mean'],
                               r['metrics']['foreground_competition']['mean'], places=6)
        # Shift source evidence and source coordinates together; wall time stays put.
        p = plan()
        for t in modified:
            for f in t['stems']['frames']:
                f['start_sec'] += 3
                f['end_sec'] += 3
            t['tempo']['beats'] = [v + 3 for v in t['tempo']['beats']]
            t['duration_sec'] += 3
        for key in ('outgoing_source', 'incoming_source'):
            p[key]['nodes'] = [(u, v + 3) for u, v in p[key]['nodes']]
        for key in ('t_in_a', 't_out_a', 't_in_b', 't_end_b'):
            p[key] += 3
        r = evaluate_pair(p, {t['track_id']: Track(t) for t in modified})
        self.assertAlmostEqual(original['audible_overlap_seconds'], r['audible_overlap_seconds'])
        self.assertAlmostEqual(original['metrics']['grid_phase_ms']['mean'], r['metrics']['grid_phase_ms']['mean'])

    def test_tempo_scaling_preserves_fractional_error(self):
        a, b = (.5, .2), (.51, .3)
        slow = grid_error(tuple(x * 2 for x in a), tuple(x * 2 for x in b))
        base = grid_error(a, b)
        self.assertAlmostEqual(base[0], slow[0])
        self.assertAlmostEqual(base[1] * 2, slow[1])

    def test_vocal_fade_and_quiet_held_note(self):
        a = track(1, voice=True)
        p = plan()
        p['lanes']['gain_a'] = curve((0, 0), (4, -30))
        tracks = {1: Track(a), 2: self.tracks[2]}
        voiced = evaluate_pair(p, tracks)['metrics']['voice_attenuation_db']
        self.assertGreater(voiced['above_seconds']['6'], 4)
        for f in a['stems']['frames']:
            f['vocal_activity'] = 0
        a['stems']['notes'] = []
        silent = evaluate_pair(p, {1: Track(a), 2: self.tracks[2]})['metrics']['voice_attenuation_db']
        self.assertIsNone(silent['mean'])
        a['stems']['notes'] = [{'stem': 'vocals', 'confidence': .9, 'start_sec': 4, 'end_sec': 13}]
        held = evaluate_pair(p, {1: Track(a), 2: self.tracks[2]})['metrics']['voice_attenuation_db']
        self.assertGreater(held['above_seconds']['6'], 4)

    def test_muted_voice_does_not_supply_instrument_chroma(self):
        raw = {'chroma': [[1] + [0] * 11, [0, 1] + [0] * 10]}
        heard = [[0, 0, 0], [0, 0, 0], [0, 1, 0], [0, 0, 0]]
        self.assertEqual(heard_chroma({'raw': raw, 'heard': heard, 'pitch': -1}), [1] + [0] * 11)
        raw['chroma'][1] = [0] * 12
        self.assertIsNone(heard_chroma({'raw': raw, 'heard': heard, 'pitch': 0}))

    def test_energy_loss_and_foreground_are_separate(self):
        p = plan()
        good = evaluate_pair(p, self.tracks)
        for s in ('a', 'b'):
            p['lanes']['gain_' + s] = curve((0, -12))
        low = evaluate_pair(p, self.tracks)
        self.assertGreater(low['metrics']['peak_deficit_db']['mean'], good['metrics']['peak_deficit_db']['mean'] + 8)
        self.assertLess(low['metrics']['foreground_competition']['mean'], good['metrics']['foreground_competition']['mean'])

    def test_natural_quiet_passage_is_not_peak_loss(self):
        quiet = copy.deepcopy(self.data)
        for t in quiet:
            for f in t['stems']['frames']:
                if 2 < f['start_sec'] < 15:
                    f['band_db'] = [[db - 24 for db in band] for band in f['band_db']]
        r = evaluate_pair(plan(), {t['track_id']: Track(t) for t in quiet})
        self.assertEqual(r['metrics']['peak_deficit_db']['eligible_seconds'], 0)

    def test_natural_attack_does_not_increase_control_rise(self):
        modified = copy.deepcopy(self.data)
        for f in modified[1]['stems']['frames']:
            if 6 < f['start_sec'] < 7:
                f['band_db'][2][1] += 12
        before = evaluate_pair(plan(), self.tracks)['metrics']['tonal_control_rise_per_second']
        after = evaluate_pair(plan(), {t['track_id']: Track(t) for t in modified})['metrics']['tonal_control_rise_per_second']
        self.assertAlmostEqual(before['max'], after['max'])

    def test_boundary_cut_has_control_rise_and_no_harmony_score(self):
        r = evaluate_pair(plan(0), self.tracks)
        self.assertIsNone(r['metrics']['tonal_roughness']['mean'])
        self.assertGreater(r['metrics']['tonal_control_rise_per_second']['max'], 1)

    def test_natural_attack_is_not_control_movement(self):
        p = plan()
        p['lanes']['xfader'] = curve((0, 1))
        r = evaluate_pair(p, self.tracks)
        # Constant B controls have no mid-transition jump, though boundary takeover
        # remains observable. Inspect only the transition's interior directly.
        model = Plan(p, self.tracks)
        old, new = model.at(3)[1][1], model.at(3.25)[1][1]
        self.assertEqual(old['gains'], new['gains'])
        self.assertGreater(r['metrics']['tonal_control_rise_per_second']['max'], 0)

    def test_loop_exclusion_and_set_solo_pitch_burden(self):
        p = plan()
        p['lanes']['loop_a'] = {'on_bar': 0}
        r = evaluate_pair(p, self.tracks)
        self.assertEqual(r['unsupported'], ['loop_a'])
        self.assertIsNone(r['metrics']['peak_deficit_db']['mean'])
        p['lanes']['loop_a'] = None
        p['incoming_offset_end']['pitch_semitones'] = 3
        r = evaluate_set([p], self.tracks)
        self.assertAlmostEqual(r['summary']['solo_pitch_semitone_seconds'], 54)

    def test_solo_restoration_does_not_extend_outgoing_voice_exposure(self):
        p = plan()
        voiced = {1: Track(track(1, voice=True)), 2: self.tracks[2]}
        base = evaluate_pair(p, voiced)
        p['clock']['nodes'].append((8, 16))
        p['incoming_source']['nodes'].append((8, 20))
        p['t_end_b'] = 20
        p['lanes']['xfader'] = curve((0, 0), (4, 0), (4.001, 1), (8, 1))
        extended = evaluate_pair(p, voiced)
        self.assertEqual(base['metrics']['voice_attenuation_db']['observed_seconds'],
                         extended['metrics']['voice_attenuation_db']['observed_seconds'])

    def test_max_pitch_includes_opening_solo_and_excludes_unheard_offsets(self):
        p = plan(0)
        p['outgoing_offset']['pitch_semitones'] = -6
        r = evaluate_set([p], self.tracks)['summary']
        self.assertEqual(r['max_abs_pitch'], 6)
        self.assertAlmostEqual(r['solo_pitch_semitone_seconds'], 24)
        silent = copy.deepcopy(self.data)
        for f in silent[0]['stems']['frames']:
            f['band_db'] = [[-120] * 3 for _ in range(3)]
        r = evaluate_set([p], {t['track_id']: Track(t) for t in silent})['summary']
        self.assertEqual(r['max_abs_pitch'], 0)
        p['t_in_a'] = 0
        p['t_end_b'] = self.tracks[2].duration
        p['incoming_offset_end']['pitch_semitones'] = 4
        r = evaluate_set([p], self.tracks)['summary']
        self.assertEqual(r['max_abs_pitch'], 0)

    def test_cue_recall_requires_independent_annotations(self):
        self.assertEqual(cue_events(None, {})['status'], 'unavailable')
        r = cue_events([{'track_id': 1, 'time_sec': 12, 'tolerance_sec': 1}], {'1': [13, 20]})
        self.assertEqual(r['recall'], 1)


if __name__ == '__main__':
    unittest.main()
