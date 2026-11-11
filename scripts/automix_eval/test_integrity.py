import copy
import unittest
from test_observer import track, plan, curve
from automix_eval.signal import Track
from automix_eval.integrity import evaluate, vocal_phrases


class IntegrityTests(unittest.TestCase):
    def voice(self, gap=(7.7, 8.3), end=14):
        a = track(1, voice=True)
        a['stems']['notes'] = []
        a['tempo']['global_bpm'] = 120
        for f in a['stems']['frames']:
            if gap[0] <= f['start_sec'] < gap[1] or f['start_sec'] >= end:
                f['band_db'][0] = [-120] * 3
                f['vocal_activity'] = 0
        return a

    def exchange(self):
        p = plan()
        p['lanes']['xfader'] = curve((0, -1), (2, -1), (2.1, 1), (4, 1))
        return p

    def test_interword_rest_cannot_hide_lost_remainder(self):
        tracks = {1: Track(self.voice()), 2: Track(track(2))}
        r = evaluate(self.exchange(), tracks)
        self.assertTrue(any(e['side'] == 0 for e in r['vocal_events']))
        self.assertIsNone(r['score'])
        self.assertEqual(r['score_status'], 'retired_uncalibrated')
        # A complete source phrase ends before the same envelope moves.
        tracks[1] = Track(self.voice(end=7.7))
        self.assertFalse(evaluate(self.exchange(), tracks)['vocal_events'])

    def test_half_double_gain_and_translation_preserve_phrase_decision(self):
        for bpm in [60, 120, 240]:
            for db in [-20, 0, 6]:
                a = self.voice()
                a['tempo']['global_bpm'] = bpm
                for f in a['stems']['frames']:
                    f['band_db'] = [[v + db for v in part] for part in f['band_db']]
                r = evaluate(self.exchange(), {1: Track(a), 2: Track(track(2))})
                self.assertTrue(r['vocal_events'])
        a = self.voice()
        shifted = copy.deepcopy(a)
        shifted['duration_sec'] += 10
        for f in shifted['stems']['frames']:
            f['start_sec'] += 10
            f['end_sec'] += 10
        self.assertEqual(vocal_phrases(Track(shifted)), [[x+10, y+10] for x,y in vocal_phrases(Track(a))])

    def test_incoming_partial_sentence_and_unknown_evidence(self):
        p = self.exchange()
        incoming = self.voice()
        incoming['track_id'] = 2
        r = evaluate(p, {1: Track(track(1)), 2: Track(incoming)})
        self.assertTrue(any(e['side'] == 1 for e in r['vocal_events']))
        missing = track(1)
        missing['stems'] = None
        self.assertIsNone(evaluate(p, {1: Track(missing), 2: Track(track(2))})['score'])

    def test_drums_do_not_mask_an_artificial_instrument_gap(self):
        p = plan()
        p['stem_mix'] = {'outgoing': [curve((0,1)) for _ in range(4)],
                         'incoming': [curve((0,1)) for _ in range(4)]}
        for side in ['outgoing','incoming']:
            p['stem_mix'][side][2] = curve((0,0))
        tracks = {1: Track(track(1)), 2: Track(track(2))}
        self.assertGreater(evaluate(p, tracks)['instrument_bed_gap_seconds'], 7)
        for t in [track(1), track(2)]:
            for f in t['stems']['frames']:
                f['band_db'][2][1:] = [-120,-120]
            tracks[t['track_id']] = Track(t)
        self.assertEqual(evaluate(p, tracks)['instrument_bed_gap_seconds'], 0)

    def test_overlapping_different_notes_are_not_one_damaged_sentence(self):
        a, b = track(1), track(2)
        # The incoming first two notes are finished before IN; the third has
        # its own attack at IN. Their small overlaps must not join them all.
        b['stems']['notes'] = [dict(stem='instruments', start_sec=lo,
            end_sec=hi, midi=pitch, confidence=.9) for lo,hi,pitch in
            [(0,2.2,71),(2,4,52),(4,6,36)]]
        p=plan(0)
        r=evaluate(p,{1:Track(a),2:Track(b)})
        self.assertFalse([e for e in r['instrument_note_events'] if e['side']==1])

    def test_silent_incoming_preroll_does_not_invent_a_lost_foreground(self):
        a, b = track(1), track(2)
        for f in a['stems']['frames']:
            # A naturally sparse instrumental foreground, still fully heard.
            f['band_db'][2][1:] = [-30, -40]
        tracks = {1: Track(a), 2: Track(b)}
        p = plan()
        p['lanes']['xfader'] = curve((0, -1))
        self.assertEqual(evaluate(p, tracks)['foreground_gap_seconds'], 0)
        # Removing an actually present outgoing foreground is still a gap.
        tracks[1] = Track(track(1))
        p['stem_mix'] = {'outgoing': [curve((0, 1)) for _ in range(4)],
                         'incoming': [curve((0, 0)) for _ in range(4)]}
        p['stem_mix']['outgoing'][2] = curve((0, 0))
        self.assertGreater(evaluate(p, tracks)['foreground_gap_seconds'], 7)

    def test_partial_attack_and_release_are_individual_control_observations(self):
        a,b=track(1),track(2)
        a['stems']['notes']=[dict(stem='instruments',start_sec=2,end_sec=6,midi=67,confidence=.9)]
        b['stems']['notes']=[dict(stem='instruments',start_sec=2,end_sec=6,midi=72,confidence=.9)]
        r=evaluate(plan(0),{1:Track(a),2:Track(b)})
        events=r['instrument_note_events']
        self.assertTrue(any(e['side']==0 and e['attenuated_release'] for e in events))
        self.assertTrue(any(e['side']==1 and e['hidden_attack'] for e in events))
        self.assertTrue(all(e['interpretation']=='review_not_phrase_damage' for e in events))
        self.assertEqual({e['midi']for e in events},{67,72})

    def test_note_control_classification_is_invariant_to_constant_level_trim(self):
        a,b=track(1),track(2)
        a['stems']['notes']=[dict(stem='instruments',start_sec=5,end_sec=11,midi=67,confidence=.9)]
        results=[]
        for trim in [-24,0,6]:
            p=self.exchange();p['lanes']['gain_a']=curve((0,trim))
            es=evaluate(p,{1:Track(a),2:Track(b)})['instrument_note_events']
            e=next(e for e in es if e['side']==0)
            self.assertTrue(e['attenuated_release'])
            results.append(e['max_power_gain_change_per_pulse'])
        self.assertAlmostEqual(min(results),max(results))


if __name__ == '__main__':
    unittest.main()
