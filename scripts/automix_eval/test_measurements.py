import copy
import math
import unittest
from test_observer import track, plan, curve
from automix_eval.signal import Plan, Track
from automix_eval.metrics import evaluate_pair, evaluate_set
from automix_eval import integrity, queue


class MeasurementTests(unittest.TestCase):
    def setUp(self):
        self.tracks = {i: Track(track(i)) for i in (1, 2)}

    def terminal(self):
        p = plan(2)
        p.update(t_in_a=28., t_out_a=30., t_end_b=6.)
        p['summary']['length_bars'] = 1
        p['clock'] = curve((0, 0), (1, 2))
        p['outgoing_source'] = curve((0, 28), (1, 30))
        p['incoming_source'] = curve((0, 4), (1, 6))
        return p

    def test_delayed_launch_and_variable_rate_inverse(self):
        p = plan()
        p.update(incoming_start_bar=2, t_end_b=8)
        p['incoming_source'] = curve()
        model = Plan(p, self.tracks)
        self.assertAlmostEqual(model.source_wall(1, 6), 6)
        self.assertAlmostEqual(model.source_wall(1, 3), 3)
        p['clock'] = curve((0, 0), (1, 1), (3, 5), (4, 8))
        p['incoming_start_bar'] = 1
        p['lanes']['rate_b'] = curve((0, 1), (4, 2))
        p['t_end_b'] = 15.625
        model = Plan(p, self.tracks)
        for u in (1, 1.5, 2, 3.5, 4):
            self.assertAlmostEqual(model.source_wall(1, model.source_at(1, u)), model.clock.at(u), places=6)
        self.assertAlmostEqual(model.source_wall(1, 17.625), 9)

    def test_short_explicit_source_curve_extends_through_recovery(self):
        p = plan()
        p['incoming_source'] = curve((0, 4), (2, 8))
        model = Plan(p, self.tracks)
        self.assertAlmostEqual(model.source_wall(1, 10), 6)
        p['summary']['length_bars'] = 0
        p['t_out_a'] = p['t_in_a']
        model = Plan(p, self.tracks)
        self.assertAlmostEqual(model.source_wall(0, 4.5), .5)

    def test_explicit_and_implicit_delayed_voice_have_identical_phrase_evidence(self):
        p = plan()
        p.update(incoming_start_bar=2, t_end_b=8)
        p['incoming_source'] = curve((0, 4), (2, 4), (4, 8))
        tracks = {1: self.tracks[1], 2: Track(track(2, voice=True))}
        explicit = integrity.evaluate(p, tracks)
        p['incoming_source'] = curve()
        implicit = integrity.evaluate(p, tracks)
        self.assertEqual(len(explicit['vocal_events']), len(implicit['vocal_events']))
        for a, b in zip(explicit['vocal_events'], implicit['vocal_events']):
            for key in a:
                self.assertAlmostEqual(a[key], b[key], places=9)

    def test_missing_tail_evidence_does_not_certify_an_active_endpoint(self):
        a = track(1)
        a['stems']['frames'] = a['stems']['frames'][:-20]
        h = evaluate_pair(self.terminal(), {1: Track(a), 2: self.tracks[2]})['handoff']
        self.assertEqual(h['source_tail']['status'], 'unknown')
        self.assertIsNone(h['heard_tail_position'])

    def test_unsupported_and_missing_evidence_do_not_become_zero_overlap(self):
        for name in ('loop_a', 'scratch_b', 'insert_a', 'send_fx', 'missing'):
            p, ts = self.terminal(), copy.deepcopy(self.tracks)
            if name in ('loop_a', 'scratch_b'):
                p['lanes'][name] = {'on_bar': 0, 'off_bar': 1}
            elif name == 'missing':
                a = track(1)
                a['stems'] = None
                ts[1] = Track(a)
            else:
                p[name] = {'enabled': True}
            r = evaluate_pair(p, ts)
            self.assertIsNone(r['handoff']['legacy_tail_short'], name)
            self.assertGreater(r['handoff']['overlap']['unknown_seconds'], 0)
            self.assertIsNone(r['handoff']['overlap']['under_four_clock_bars'])
            m = r['metrics']['grid_phase_ms']
            self.assertGreater(m['eligible_seconds'], m['observed_seconds'])

    def test_silent_padding_and_clock_alias_do_not_change_seconds_or_active_tail(self):
        p = self.terminal()
        a = evaluate_pair(p, self.tracks)['handoff']
        padded = track(1, duration=60)
        for f in padded['stems']['frames']:
            if f['start_sec'] >= 30:
                f['band_db'] = [[-120] * 3 for _ in range(3)]
        b = evaluate_pair(p, {1: Track(padded), 2: self.tracks[2]})['handoff']
        for key in ('heard_tail_position', 'remaining_active_source_seconds', 'remaining_to_active_end_seconds'):
            self.assertEqual(a[key], b[key])
        q = copy.deepcopy(p)
        q['summary']['length_bars'] /= 2
        for key in ('clock', 'incoming_source', 'outgoing_source'):
            q[key]['nodes'] = [(u / 2, v) for u, v in q[key]['nodes']]
        c = evaluate_pair(q, self.tracks)['handoff']
        self.assertEqual(a['overlap']['seconds_lower'], c['overlap']['seconds_lower'])
        self.assertEqual(a['overlap']['longest_run_seconds'], c['overlap']['longest_run_seconds'])
        self.assertAlmostEqual(a['overlap']['bars_lower'], c['overlap']['bars_lower'] * 2)

    def test_fragmented_overlap_and_brief_rest_are_distinguished(self):
        p = plan(16)
        p['summary']['length_bars'] = 8
        p['clock'] = curve((0, 0), (8, 16))
        p.update(t_in_a=14., t_out_a=30., t_end_b=20.)
        p['outgoing_source'] = curve((0, 14), (8, 30))
        p['incoming_source'] = curve((0, 4), (8, 20))
        p['lanes']['xfader'] = curve((0, 0), (2, 0), (2.001, -1), (5.999, -1), (6, 0), (8, 0))
        h = evaluate_pair(p, self.tracks)['handoff']['overlap']
        self.assertAlmostEqual(h['seconds_lower'], 8)
        self.assertAlmostEqual(h['longest_run_seconds'], 4)
        self.assertAlmostEqual(h['gap_tolerant_longest_seconds'], 4)
        p = plan()
        p['lanes']['xfader'] = curve((0, 0), (2, 0), (2.001, -1), (2.049, -1), (2.05, 0), (4, 0))
        h = evaluate_pair(p, self.tracks)['handoff']['overlap']
        self.assertLess(h['longest_run_seconds'], 5)
        self.assertAlmostEqual(h['gap_tolerant_longest_seconds'], 8)

    def test_unknown_gaps_are_never_bridged(self):
        a = track(1)
        a['stems']['frames'] = [f for f in a['stems']['frames'] if not 7.95 <= f['start_sec'] < 8.05]
        h = evaluate_pair(plan(), {1: Track(a), 2: self.tracks[2]})['handoff']
        self.assertEqual(h['status'], 'partial')
        self.assertGreater(h['overlap']['unknown_seconds'], .05)
        self.assertIsNone(h['overlap']['gap_tolerant_longest_seconds'])

    def test_continuity_gap_uses_source_meter_then_real_playback_rate(self):
        p = plan()
        a = evaluate_pair(p, self.tracks)['handoff']['overlap']
        p['lanes']['rate_a'] = curve((0, 2))
        b = evaluate_pair(p, self.tracks)['handoff']['overlap']
        self.assertAlmostEqual(a['gap_tolerance_seconds'], 2 * b['gap_tolerance_seconds'])
        aliases = copy.deepcopy(track(1))
        aliases['tempo']['beats'] = [i / 4 for i in range(120)]
        aliases['tempo']['segments'][0]['end_beat'] = 120
        c = evaluate_pair(p, {1: Track(aliases), 2: self.tracks[2]})['handoff']['overlap']
        self.assertAlmostEqual(b['gap_tolerance_seconds'], c['gap_tolerance_seconds'])

    def test_voice_carry_is_separate_from_foreground_gap_and_phrase_damage(self):
        p, a = plan(), track(1, voice=True)
        for f in a['stems']['frames']:
            if f['start_sec'] >= 12:
                f['band_db'][0] = [-120] * 3
                f['vocal_activity'] = 0
        for n in a['stems']['notes']:
            if n['stem'] == 'vocals':
                n['end_sec'] = 12
        p['lanes']['gain_a'] = curve((0, 10 * math.log10(2)))
        p['stem_mix'] = {'outgoing': [curve((0, float(s in (0, 1)))) for s in range(4)],
                         'incoming': [curve((0, float(s == 1))) for s in range(4)]}
        r = integrity.evaluate(p, {1: Track(a), 2: self.tracks[2]})
        self.assertEqual(r['vocal_events'], [])
        self.assertIsNone(r['score'])
        self.assertGreater(r['voice_carried_bed_gap_seconds'], 7)
        self.assertEqual(r['foreground_gap_seconds'], 0)
        p['lanes']['gain_a'] = curve((0, 3), (2, 3), (2.1, -96), (4, -96))
        r = integrity.evaluate(p, {1: Track(a), 2: self.tracks[2]})
        self.assertTrue(r['vocal_events'])
        self.assertGreater(r['foreground_gap_seconds'], 3)

    def test_sustained_note_loss_is_observed_even_with_another_foreground(self):
        p = plan()
        p['stem_mix'] = {'outgoing': [curve((0, 1)) for _ in range(4)],
                         'incoming': [curve((0, 1)) for _ in range(4)]}
        p['lanes']['gain_a'] = curve((0, 3))
        p['stem_mix']['outgoing'][2] = curve((0, 1), (2, 1), (2.1, 0), (4, 0))
        r = integrity.evaluate(p, {1: self.tracks[1], 2: Track(track(2, voice=True))})
        self.assertTrue(r['sustained_instrument_events'])

    def test_occurrences_and_context_edge_are_not_extra_cycle_departures(self):
        plans = [plan(), plan(), plan()]
        for p in plans:
            p['summary']['pair'] = [1, 1]
        result = evaluate_set(plans, self.tracks, evaluated_pairs=2)
        run = {'cycle_size': 2, 'queue_entries': [
            {'entry_id': name, 'track_id': 1} for name in ('a', 'b', 'a', 'b')]}
        queue.annotate(result, plans, run)
        self.assertEqual(result['summary']['pairs'], 2)
        self.assertEqual(result['queue_coverage']['departures'], {'a': 1, 'b': 1})
        self.assertEqual(result['queue_coverage']['context_pairs'], 1)
        self.assertTrue(result['cycles'][0]['complete'])
        self.assertTrue(result['pairs'][1]['occurrence']['wraparound'])
        run['queue_entries'][1]['entry_id'] = 'a'
        with self.assertRaises(ValueError):
            queue.annotate(result, plans, run)


if __name__ == '__main__':
    unittest.main()
