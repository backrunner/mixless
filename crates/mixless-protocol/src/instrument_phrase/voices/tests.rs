use super::*;
use crate::StemKind;

fn polyphonic() -> TrackAnalysis {
    let mut t = super::super::tests::recording();
    let s = t.stems.as_mut().unwrap();
    s.notes.clear();
    for i in 0..24 {
        for midi in [[36, 38, 43, 36][i % 4], [72, 74, 76, 74][i % 4]] {
            s.notes.push(StemNote {
                stem: StemKind::Instruments,
                start_sec: i as f32,
                end_sec: i as f32 + 0.8,
                midi,
                confidence: 0.9,
            });
        }
    }
    t
}

#[test]
fn bass_and_upper_line_remain_distinct_without_requiring_isolated_stems() {
    let t = polyphonic();
    let voices = instrument_voices(&t);
    assert_eq!(voices.len(), 2);
    assert!(voices.iter().all(|v| v.notes.len() == 24));
    assert!(voices[0].register < 45. && voices[1].register > 70.);
    for (index, v) in voices.iter().enumerate() {
        assert!(v.notes.iter().all(|n| (n.midi >= 60) == (index == 1)));
    }
}

#[test]
fn a_held_pad_cannot_steal_the_notes_of_an_articulated_line() {
    let mut t = polyphonic();
    t.stems.as_mut().unwrap().notes.push(StemNote {
        stem: StemKind::Instruments,
        start_sec: 0.,
        end_sec: 24.,
        midi: 60,
        confidence: 0.9,
    });
    t.stems
        .as_mut()
        .unwrap()
        .notes
        .sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    let voices = instrument_voices(&t);
    assert_eq!(voices.len(), 3);
    assert_eq!(
        voices
            .iter()
            .find(|v| v.notes[0].midi == 60)
            .unwrap()
            .notes
            .len(),
        1
    );
}

#[test]
fn contour_can_continue_across_register_crossings() {
    let mut t = polyphonic();
    let s = t.stems.as_mut().unwrap();
    s.notes.clear();
    for (i, pitches) in [[60, 69], [62, 67], [64, 65], [66, 63], [68, 61]]
        .iter()
        .enumerate()
    {
        for &midi in pitches {
            s.notes.push(StemNote {
                stem: StemKind::Instruments,
                start_sec: i as f32,
                end_sec: i as f32 + 0.8,
                midi,
                confidence: 0.9,
            });
        }
    }
    let voices = instrument_voices(&t);
    assert_eq!(voices.len(), 2);
    assert_eq!(
        voices[0].notes.iter().map(|n| n.midi).collect::<Vec<_>>(),
        [60, 62, 64, 66, 68]
    );
    assert_eq!(
        voices[1].notes.iter().map(|n| n.midi).collect::<Vec<_>>(),
        [69, 67, 65, 63, 61]
    );
}

#[test]
fn transposition_gain_time_and_storage_order_do_not_reassign_voices() {
    let original = polyphonic();
    for speed in [0.8, 1., 1.2] {
        let mut t = original.clone();
        t.duration_sec = t.duration_sec / speed;
        t.tempo.global_bpm *= speed;
        for beat in &mut t.tempo.beats {
            *beat /= speed;
        }
        let s = t.stems.as_mut().unwrap();
        s.duration_sec = t.duration_sec;
        for n in &mut s.notes {
            n.start_sec /= speed;
            n.end_sec /= speed;
            n.midi += 5;
        }
        for f in &mut s.frames {
            f.start_sec /= speed;
            f.end_sec /= speed;
            f.rms = f.rms.map(|v| v * 0.1);
        }
        s.notes.sort_by(|a, b| {
            a.start_sec
                .total_cmp(&b.start_sec)
                .then(b.midi.cmp(&a.midi))
        });
        let voices = instrument_voices(&t);
        assert_eq!(voices.len(), 2);
        for (v, old) in voices.iter().zip(instrument_voices(&original)) {
            assert_eq!(v.notes.len(), old.notes.len());
            assert!(v
                .notes
                .iter()
                .zip(old.notes)
                .all(|(a, b)| a.midi == b.midi + 5));
        }
    }
}

#[test]
fn assignment_is_joint_and_marks_close_alternatives_as_uncertain() {
    assert_eq!(assignment(&[vec![1., 2.], vec![1.1, 10.]]), [1, 0]);
    let mut t = polyphonic();
    for n in &mut t.stems.as_mut().unwrap().notes {
        n.midi = 60 + (n.midi % 2);
    }
    assert!(instrument_voices(&t).iter().any(|v| v.ambiguous_links > 0));
}

#[test]
fn a_repeating_bass_cannot_outvote_a_sparse_evolving_upper_voice() {
    let mut t = polyphonic();
    assert!(!crate::instrument_boundaries(&t).is_empty());
    let s = t.stems.as_mut().unwrap();
    s.notes.retain(|n| n.midi < 60);
    // Bass supplies most notes; the few foreground notes still participate.
    for (i, pitch) in [72, 74, 77, 79, 81, 83].iter().enumerate() {
        s.notes.push(StemNote {
            stem: StemKind::Instruments,
            start_sec: i as f32 * 4.,
            end_sec: i as f32 * 4. + 3.8,
            midi: *pitch,
            confidence: 0.9,
        });
    }
    s.notes.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    assert!(crate::instrument_boundaries(&t).is_empty());
}
