import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from automix_eval import realism
from automix_eval.comparison import compare
from automix_eval.metrics import evaluate_set
from automix_eval.signal import Track
from automix_eval.test_observer import curve, plan, track


def following(rate=1.):
    """Track 2 leaves from the state the first transition gave it."""
    p = plan()
    p['summary']['pair'] = [2, 1]
    p['t_in_a'], p['t_out_a'] = 14, 22
    p['outgoing_source'] = curve((0, 14), (4, 22))
    p['outgoing_offset'] = {'rate': rate, 'pitch_semitones': 0}
    p['lanes']['rate_a'] = curve((0, rate))
    return p


def held(rate=1., pitch=0.):
    p = plan()
    p['lanes']['rate_b'] = curve((0, rate))
    p['lanes']['pitch_b'] = curve((0, pitch))
    p['incoming_offset_end'] = {'rate': rate, 'pitch_semitones': pitch}
    return p


class RealismTests(unittest.TestCase):
    def setUp(self):
        self.use()

    def use(self, bpms=(120, 120), keys=('8A', '8A')):
        data = [track(1), track(2)]
        for t, bpm, key in zip(data, bpms, keys):
            t['tempo']['global_bpm'], t['camelot'], t['key_confidence'] = bpm, key, .9
        self.tracks = {t['track_id']: Track(t) for t in data}

    def run_plans(self, plans):
        return realism.annotate(evaluate_set(plans, self.tracks), plans, self.tracks)

    def level(self, result, index=0, name='default'):
        return result['pairs'][index]['realism']['levels'][name]

    def test_aligned_native_blend_is_clean_and_matches_reference_shares(self):
        result = self.run_plans([plan()])
        for name in realism.LEVELS:
            self.assertEqual(self.level(result, name=name)['status'], 'clean')
        summary = result['realism']
        self.assertEqual(summary['levels']['default']['clean_rate'], 1)
        self.assertAlmostEqual(result['pairs'][0]['realism']['score'], 100)
        self.assertAlmostEqual(summary['score'], 100)
        self.assertEqual(summary['queue']['native_key_clash_pairs'], 0)
        self.assertEqual(summary['dj_reference']['played_tracks'], 2)
        self.assertEqual(summary['dj_reference']['transposed']['tracks'], 0)
        self.assertEqual(summary['dj_reference']['tempo_within']['0.05']['share'], 1)

    def test_held_tempo_is_charged_once_to_the_transition_that_set_it(self):
        result = self.run_plans([held(rate=1.12), following(1.12)])
        self.assertIn('tempo', self.level(result)['deviating'])
        self.assertNotIn('tempo', self.level(result, name='lenient')['deviating'])
        self.assertNotIn('tempo', self.level(result, 1)['deviating'])
        self.assertAlmostEqual(result['pairs'][0]['realism']['tempo_adjustment'], .12)
        played = {t['track_id']: t for t in result['realism']['played_tracks'][:2]}
        self.assertLess(played[1]['tempo_adjustment'], .001)
        self.assertAlmostEqual(played[2]['tempo_adjustment'], .12, places=3)
        within = result['realism']['dj_reference']['tempo_within']['0.1']
        self.assertEqual(within['tracks_outside'], 1)
        self.assertLess(within['chance_under_reference'], .2)

    def test_moving_the_outgoing_tempo_is_charged(self):
        p = plan()
        p['lanes']['rate_a'] = curve((0, 1), (4, 1.12))
        result = self.run_plans([p])
        self.assertIn('tempo', self.level(result)['deviating'])

    def test_transposition_levels_follow_semitones(self):
        one = self.run_plans([held(pitch=1)])
        self.assertNotIn('key', self.level(one)['deviating'])
        self.assertIn('key', self.level(one, name='strict')['deviating'])
        three = self.run_plans([held(pitch=-3)])
        for name in realism.LEVELS:
            self.assertIn('key', self.level(three, name=name)['deviating'])
        shifted = three['realism']['dj_reference']['transposed']
        self.assertEqual((shifted['tracks'], shifted['max_semitones']), (1, 3))
        self.assertEqual(shifted['one_semitone_share'], 0)
        self.assertGreater(three['realism']['dj_reference']['mean_gap_percentage_points'], 10)

    def test_unobserved_processing_is_unknown_but_cannot_hide_a_deviation(self):
        p = plan()
        p['lanes']['loop_a'] = {'start': 0}
        result = self.run_plans([p])
        self.assertEqual(self.level(result)['status'], 'unknown')
        level = result['realism']['levels']['default']
        self.assertIsNone(level['clean_rate'])
        self.assertEqual(level['clean_rate_bounds'], [0, 1])
        p = held(pitch=4)
        p['lanes']['loop_a'] = {'start': 0}
        result = self.run_plans([p])
        self.assertEqual(self.level(result)['status'], 'deviating')
        self.assertEqual(result['realism']['dj_reference']['transposed']['tracks'], 1)

    def test_off_grid_overlap_deviates(self):
        p = plan()
        p['incoming_source'] = curve((0, 4.125), (4, 12.125))
        p['t_in_b'], p['t_end_b'] = 4.125, 12.125
        self.assertIn('beat_sync', self.level(self.run_plans([p]))['deviating'])

    def test_overlap_descriptors_separate_reserved_and_heard_bars(self):
        p = plan()
        p['incoming_start_bar'] = 3
        overlap = self.run_plans([p, following()])['realism']['overlap']
        self.assertEqual(overlap['nominal_blend_pairs'], 2)
        self.assertEqual(overlap['heard_blend_pairs'], 1)
        cut = self.run_plans([plan(0)])['realism']['overlap']
        self.assertEqual((cut['nominal_blend_pairs'], cut['heard_blend_pairs']), (0, 0))

    def test_tempo_the_queue_forces_is_not_charged(self):
        self.assertIn('tempo', self.level(self.run_plans([held(rate=1.12)]))['deviating'])
        self.use(bpms=(120, 150))
        result = self.run_plans([held(rate=1.12)])
        pair = result['pairs'][0]['realism']
        self.assertAlmostEqual(pair['forced_tempo_adjustment'], 1.25 ** .5 - 1)
        self.assertLess(pair['tempo_excess'], .01)
        self.assertEqual(pair['credits']['tempo'], 1)
        self.assertEqual(result['realism']['queue']['forced_tempo_over_5_percent_pairs'], 1)
        # Half and double time need no stretch.
        self.use(bpms=(87, 174))
        self.assertLess(self.run_plans([plan()])['pairs'][0]['realism']['forced_tempo_adjustment'], 1e-6)

    def test_clashing_native_keys_allow_one_more_semitone(self):
        self.assertIn('key', self.level(self.run_plans([held(pitch=2)]))['deviating'])
        for compatible in ('8A', '8B', '9A', '7A'):
            self.use(keys=('8A', compatible))
            self.assertFalse(self.run_plans([plan()])['pairs'][0]['realism']['native_key_clash'])
        self.use(keys=('8A', '3B'))
        result = self.run_plans([held(pitch=2)])
        self.assertTrue(result['pairs'][0]['realism']['native_key_clash'])
        self.assertNotIn('key', self.level(result)['deviating'])
        self.assertIn('key', self.level(self.run_plans([held(pitch=6)]), name='lenient')['deviating'])
        self.use(keys=('8A', None))
        result = self.run_plans([held(pitch=2)])
        self.assertIsNone(result['pairs'][0]['realism']['native_key_clash'])
        self.assertIn('key', self.level(result)['deviating'])

    def test_score_rewards_clean_transitions_and_keeps_partial_credit(self):
        def score(passed):
            """`passed` maps a check to the tightest level it passes, or None."""
            order = list(realism.LEVELS)
            values = {name: {c: True for c in realism.CHECKS} for name in order}
            for check, level in passed.items():
                for name in order[:order.index(level) if level else len(order)]:
                    values[name][check] = False
            levels = {name: {'status': 'deviating' if False in values[name].values() else 'clean'}
                      for name in order}
            return realism.score(levels, {c: realism.credit(values, c) for c in realism.CHECKS})
        strict, default, lenient, failed = (score({'key': v}) for v in ('strict', 'default', 'lenient', None))
        n = len(realism.CHECKS)
        self.assertAlmostEqual(strict, 100)
        self.assertAlmostEqual(default, 100 * (.6 * (n - .2) / n + .4))
        self.assertAlmostEqual(lenient, 100 * (.6 * (n - .5) / n + .2))
        self.assertAlmostEqual(failed, 100 * .6 * (n - 1) / n)
        # Losing the clean status costs more than a looser pass of one check.
        self.assertGreater(default - lenient, strict - default)
        self.assertGreater(failed, 50)
        self.assertGreater(score({'key': 'default', 'energy': 'default'}), lenient)
        self.assertAlmostEqual(score({c: None for c in realism.CHECKS}), 0)

    def test_measured_transposition_orders_the_score(self):
        self.use(keys=('8A', '3B'))
        scores = [self.run_plans([held(pitch=v)])['pairs'][0]['realism']['credits']['key'] for v in (1, 2, 3, 6)]
        self.assertEqual(scores, [1, .8, .5, 0])

    def test_unscored_transitions_bound_the_run_score(self):
        looped = following()
        looped['lanes']['loop_a'] = {'start': 0}
        result = self.run_plans([plan(), looped])
        summary = result['realism']
        self.assertIsNone(result['pairs'][1]['realism']['score'])
        self.assertEqual((summary['scored_transitions'], summary['unscored_transitions']), (1, 1))
        self.assertAlmostEqual(summary['score'], 100)
        self.assertEqual([round(v) for v in summary['score_bounds']], [50, 100])
        # A deviation known at every level is scored on the observed checks.
        failed = held(pitch=6)
        failed['lanes']['loop_a'] = {'start': 0}
        # Structure and tempo pass; the shift fails both key and settle.
        self.assertAlmostEqual(self.run_plans([failed])['pairs'][0]['realism']['score'], 100 * .6 * 2 / 4)

    def test_settle_judges_what_the_track_keeps_after_the_plan(self):
        self.use(bpms=(120, 150))
        held_plan = self.run_plans([held(rate=1.12)])['pairs'][0]['realism']
        # The pair forced the stretch, but nothing justifies keeping it.
        self.assertEqual(held_plan['credits']['tempo'], 1)
        self.assertEqual(held_plan['credits']['settle'], 0)
        self.assertAlmostEqual(held_plan['kept_tempo_excess'], .12, places=5)
        restored = held(rate=1.12)
        restored['lanes']['rate_b'] = curve((0, 1.12), (4, 1.12), (8, 1))
        restored['clock'] = curve((0, 0), (4, 8), (8, 16))
        restored['incoming_offset_end'] = {'rate': 1, 'pitch_semitones': 0}
        restored = self.run_plans([restored])['pairs'][0]['realism']
        self.assertEqual(restored['credits']['settle'], 1)
        self.assertAlmostEqual(restored['tempo_adjustment'], .12, places=5)

    def test_settle_allows_an_offset_towards_the_next_track(self):
        # Track 2 keeps +5%; the next track (1) is 12% faster, so half is useful.
        self.use(bpms=(134.4, 120))
        towards = self.run_plans([held(rate=1.05), following(1.05)])['pairs'][0]['realism']
        self.assertLess(towards['kept_tempo_excess'], .001)
        self.assertEqual(towards['credits']['settle'], 1)
        # The same offset away from a slower next track is all excess.
        self.use(bpms=(107, 120))
        away = self.run_plans([held(rate=1.05), following(1.05)])['pairs'][0]['realism']
        self.assertAlmostEqual(away['kept_tempo_excess'], .05, places=5)
        self.assertEqual(away['credits']['settle'], .8)

    def test_settle_charges_a_kept_transposition_without_allowance(self):
        self.use(keys=('8A', '3B'))
        kept = self.run_plans([held(pitch=2)])['pairs'][0]['realism']
        self.assertEqual((kept['credits']['key'], kept['credits']['settle']), (.8, .5))
        back = held(pitch=2)
        back['lanes']['pitch_b'] = curve((0, 2), (4, 2), (8, 0))
        back['clock'] = curve((0, 0), (4, 8), (8, 16))
        back['incoming_offset_end'] = {'rate': 1, 'pitch_semitones': 0}
        back = self.run_plans([back])['pairs'][0]['realism']
        self.assertEqual((back['credits']['key'], back['credits']['settle']), (.8, 1))

    def test_a_cut_is_not_charged_for_replacing_the_tonal_parts_below_strict(self):
        cut = self.run_plans([plan(0)])['pairs'][0]
        self.assertEqual(cut['integrity']['abrupt_tonal_loss'], 1)
        self.assertEqual(cut['realism']['credits']['energy'], .8)
        self.assertEqual(cut['realism']['levels']['default']['status'], 'clean')
        self.assertIn('energy', cut['realism']['levels']['strict']['deviating'])
        # The same loss inside an overlap is a deviation at every level.
        blend = evaluate_set([plan()], self.tracks)
        blend['pairs'][0]['integrity']['abrupt_tonal_loss'] = 1
        blend = realism.annotate(blend, [plan()], self.tracks)['pairs'][0]['realism']
        self.assertEqual(blend['credits']['energy'], 0)

    def test_a_record_played_out_to_its_tail_is_not_a_clean_mix(self):
        # Track 1 lasts 30 s; leaving at 29 s with no overlap plays it out.
        late = plan(0)
        late['t_in_a'] = late['t_out_a'] = 29
        late['outgoing_source'] = curve((0, 29), (4, 29))
        result = self.run_plans([late])['pairs'][0]['realism']
        self.assertTrue(result['played_out'])
        self.assertEqual(result['credits']['exit'], .5)
        self.assertIn('exit', result['levels']['default']['deviating'])
        self.assertEqual(result['levels']['lenient']['status'], 'clean')
        # The same cut in the middle of the record is a cut, not a play-out.
        early = self.run_plans([plan(0)])['pairs'][0]['realism']
        self.assertFalse(early['played_out'])
        self.assertEqual(early['credits']['exit'], 1)
        # A real overlap that ends at the tail mixes the ending.
        blend = plan()
        blend['t_in_a'], blend['t_out_a'] = 21, 29
        blend['outgoing_source'] = curve((0, 21), (4, 29))
        self.assertFalse(self.run_plans([blend])['pairs'][0]['realism']['played_out'])
        # A looped source cannot be followed.
        looped = plan()
        looped['lanes']['loop_a'] = {'start': 0}
        self.assertIsNone(self.run_plans([looped])['pairs'][0]['realism']['played_out'])
        summary = self.run_plans([late, following()])['realism']['overlap']
        self.assertEqual((summary['cuts'], summary['played_out_pairs']), (1, 1))

    def test_an_unlocked_deck_counts_its_speed_as_its_transposition(self):
        semitone = 2 ** (1 / 12)
        unlocked = held(rate=semitone)
        unlocked['incoming_varispeed'] = True
        unlocked['incoming_offset_end']['pitch_semitones'] = 1
        result = self.run_plans([unlocked])['pairs'][0]['realism']
        self.assertAlmostEqual(result['transposition_semitones'], 1, places=3)
        self.assertEqual(result['kept_semitones'], 1)
        # Gliding back to the recorded speed returns the pitch with it.
        back = held(rate=semitone)
        back['incoming_varispeed'] = True
        back['lanes']['rate_b'] = curve((0, semitone), (4, semitone), (8, 1))
        back['clock'] = curve((0, 0), (4, 8), (8, 16))
        back['incoming_offset_end'] = {'rate': 1, 'pitch_semitones': 0}
        result = self.run_plans([back])['pairs'][0]['realism']
        self.assertAlmostEqual(result['transposition_semitones'], 1, places=3)
        self.assertEqual(result['kept_semitones'], 0)
        self.assertEqual(result['credits']['settle'], 1)
        played = self.run_plans([back])['realism']['played_tracks'][1]
        self.assertAlmostEqual(played['transposition_semitones'], 1, places=3)

    def test_binomial_tail(self):
        self.assertEqual(realism.tail(0, 5, .025), 1)
        self.assertAlmostEqual(realism.tail(1, 1, .025), .025)
        self.assertAlmostEqual(realism.tail(2, 2, .5), .25)

    def test_comparison_flags_more_deviating_transitions(self):
        before = self.run_plans([plan()])
        after = self.run_plans([held(pitch=3)])
        for run in (before, after):
            run['queue_coverage'] = {}
        review = compare(before, after, .05)
        self.assertIn('realism_deviating_transitions', review['alerts'])
        self.assertEqual(review['realism']['levels']['default']['deviating'], {'before': 0, 'after': 1})
        self.assertIn('realism_score', review['alerts'])
        self.assertGreater(review['realism']['score']['before'], review['realism']['score']['after'])
        same = compare(before, copy.deepcopy(before), .05)
        self.assertNotIn('realism_deviating_transitions', same['alerts'])
        self.assertNotIn('realism_score', same['alerts'])
        del before['realism']
        self.assertIsNone(compare(before, after, .05)['realism'])


if __name__ == '__main__':
    unittest.main()
