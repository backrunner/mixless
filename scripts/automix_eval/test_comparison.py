import copy
import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from automix_eval.comparison import compare_reports
from automix_eval.metrics import VERSION


def report():
    integrity = {'status': 'observed', 'vocal_damage_pairs': 0, 'instrument_bed_gap_seconds': 1.,
                 'foreground_gap_seconds': 0., 'voice_carried_bed_gap_seconds': 0.,
                 'sustained_instrument_risk_pairs': 0, 'partial_pairs': 0,
                 'unknown_phrase_source_seconds': 0., 'eligible_seconds': 8., 'observed_seconds': 8.,
                 'abrupt_tonal_loss_pairs': 0, 'unavailable_pairs': 0}
    handoff = {'legacy_tail_short': True, 'legacy_tail_short_candidates': 2, 'overlap_unknown_seconds': 0.,
               'unknown_heard_endpoint_pairs': 0, 'unavailable_pairs': 0}
    pair = {'pair': [1, 2], 'handoff': handoff, 'integrity': integrity,
            'vocal_grouping': {str(i): {'status': 'observed', 'vocal_events': []} for i in (1, 2, 3)}}
    return {'observer_version': VERSION, 'step_seconds': .05, 'normalization': 'fixed',
            'input_sha256': {'analysis.json': 'reference', 'plans.json': 'plans'},
            'runs': [{'name': 'single', 'pairs': [pair, copy.deepcopy(pair)],
                      'summary': {'pairs': 2, 'handoff': handoff,
                                  'violation_pairs': 0, 'integrity': integrity},
                      'vocal_grouping_sensitivity': {str(i): copy.deepcopy(integrity) for i in (1, 2, 3)}}]}


class ComparisonTests(unittest.TestCase):
    def test_harmonic_or_rhythmic_exposure_is_not_hidden_by_a_better_gap(self):
        a, b = report(), report()
        metric = {'eligible_seconds': 4., 'observed_seconds': 4., 'above_seconds': {'30': .2}}
        for r in (a, b):
            r['runs'][0]['summary']['metrics'] = {'grid_phase_ms': copy.deepcopy(metric)}
        b['runs'][0]['summary']['metrics']['grid_phase_ms']['above_seconds']['30'] = .4
        b['runs'][0]['summary']['integrity']['instrument_bed_gap_seconds'] = 0.
        result = compare_reports(a, b, 'reference')
        self.assertIn('signal_exposure', result['alerts'])
        self.assertEqual(result['signal_review'][0]['reason'], 'increased_exposure')

    def test_opposing_pair_changes_and_missing_harmony_evidence_remain_visible(self):
        a, b = report(), report()
        metric = {'eligible_seconds': 1., 'observed_seconds': 1., 'above_seconds': {'.4': .2}}
        for r in (a, b):
            for pair in r['runs'][0]['pairs']:
                pair['metrics'] = {'tonal_roughness': copy.deepcopy(metric)}
        b['runs'][0]['pairs'][0]['metrics']['tonal_roughness']['above_seconds']['.4'] = .4
        b['runs'][0]['pairs'][1]['metrics']['tonal_roughness'].update(observed_seconds=.8, above_seconds={'.4': 0.})
        result = compare_reports(a, b, 'reference')
        self.assertIn('pair_exposure', result['alerts'])
        self.assertEqual([p['index'] for p in result['pair_review']], [0, 1])
        self.assertEqual(result['pair_review'][1]['changes'][0]['reason'], 'increased_unknown_exposure')

    def test_terminal_count_is_a_descriptor_not_a_quality_gate(self):
        a, b = report(), report()
        a['runs'][0]['summary']['handoff']['legacy_tail_short_candidates'] = 1
        b['runs'][0]['summary']['integrity']['mean_score'] = 99
        result = compare_reports(a, b, 'reference')
        self.assertEqual(result['alerts'], [])
        self.assertEqual(result['descriptors']['handoff']['after']['legacy_tail_short_candidates'], 2)
        self.assertEqual([p['index'] for p in result['terminal_after']], [0, 1])

    def test_missing_observations_are_not_an_improvement(self):
        a, b = report(), report()
        b['runs'][0]['summary']['integrity'].update(unavailable_pairs=1, instrument_bed_gap_seconds=0)
        self.assertIn('unavailable_pairs', compare_reports(a, b, 'reference')['alerts'])

    def test_unchanged_missing_evidence_is_explicitly_incomplete(self):
        a, b = report(), report()
        for r in (a, b):
            r['runs'][0]['summary']['integrity']['partial_pairs'] = 1
        self.assertEqual(compare_reports(a, b, 'reference')['status'], 'incomplete_evidence')

    def test_instrument_bed_and_voice_bridge_are_descriptors(self):
        a, b = report(), report()
        b['runs'][0]['summary']['integrity'].update(
            instrument_bed_gap_seconds=10., voice_carried_bed_gap_seconds=10.)
        r = compare_reports(a, b, 'reference')
        self.assertEqual(r['alerts'], [])
        self.assertEqual(r['descriptors']['voice_carried_bed_gap_seconds']['after'], 10.)

    def test_merged_held_notes_are_not_a_phrase_damage_gate(self):
        a,b=report(),report()
        b['runs'][0]['summary']['integrity']['sustained_instrument_risk_pairs']=2
        b['runs'][0]['pairs'][0]['integrity']['sustained_instrument_events']=[{'lost_energy_fraction':.5}]
        self.assertEqual(compare_reports(a,b,'reference')['alerts'],[])

    def test_new_individual_attack_loss_requests_review_without_claiming_phrase_damage(self):
        a,b=report(),report()
        b['runs'][0]['pairs'][0]['integrity']['instrument_note_events']=[dict(side=1,
            source_start=2.,source_end=6.,midi=72,hidden_attack=True,
            attenuated_release=False,max_power_gain_change_per_pulse=1.)]
        r=compare_reports(a,b,'reference')
        self.assertEqual(r['pair_review'][0]['changes'][0]['reason'],'note_control_change_requires_review')

    def test_matching_obsolete_reports_and_changed_duplicate_coverage_are_rejected(self):
        a, b = report(), report()
        a['observer_version'] = b['observer_version'] = 4
        with self.assertRaises(ValueError):
            compare_reports(a, b, 'reference')
        a, b = report(), report()
        b['runs'][0]['pairs'][1]['pair'] = [2, 1]
        with self.assertRaises(ValueError):
            compare_reports(a, b, 'reference')

    def test_per_pair_vocal_damage_cannot_hide_in_an_unchanged_total(self):
        a, b = report(), report()
        b['runs'][0]['pairs'][0]['integrity']['vocal_events'] = [{'lost_energy_fraction': .8}]
        self.assertIn('pair_exposure', compare_reports(a, b, 'reference')['alerts'])

    def test_wide_grouping_pair_risk_cannot_hide_in_an_unchanged_total(self):
        a, b = report(), report()
        b['runs'][0]['pairs'][0]['vocal_grouping']['3']['vocal_events'] = [{'lost_energy_fraction': .8}]
        r = compare_reports(a, b, 'reference')
        self.assertEqual(r['pair_review'][0]['changes'][0]['metric'], 'vocal_grouping_3')

    def test_reordered_queues_and_sensitivity_keep_their_own_evidence(self):
        a, b = report(), report()
        a['runs'][0]['pairs'][1]['pair'] = [2, 1]
        b['runs'][0]['pairs'][0]['pair'] = [2, 1]
        b['runs'][0]['vocal_grouping_sensitivity']['3']['vocal_damage_pairs'] = 1
        result = compare_reports(a, b, 'reference')
        self.assertFalse(result['same_adjacencies'])
        self.assertIn('vocal_grouping_3', result['alerts'])

    def test_resolution_does_not_hide_a_material_foreground_gap(self):
        a, b = report(), report()
        b['runs'][0]['summary']['integrity']['foreground_gap_seconds'] += .05
        self.assertEqual(compare_reports(a, b, 'reference')['status'], 'no_regression_detected')
        b['runs'][0]['summary']['integrity']['foreground_gap_seconds'] += .05
        self.assertIn('foreground_gap_seconds', compare_reports(a, b, 'reference')['alerts'])

    def test_mismatched_observer_reference_and_incomplete_queue_are_rejected(self):
        for mutate in [lambda r: r.update(observer_version=3),
                       lambda r: r.update(input_sha256={'analysis.json': 'other'}),
                       lambda r: r['runs'][0]['summary'].update(pairs=1),
                       lambda r: r['runs'][0]['pairs'][0].update(pair=[1, 3])]:
            a, b = report(), report()
            mutate(b)
            with self.assertRaises(ValueError):
                compare_reports(a, b, 'reference')
