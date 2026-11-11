use super::*;
use crate::{StemAnalysis, StemFrame, TempoMap, TempoSegment, TrackId};

pub(super) fn recording() -> TrackAnalysis {
    let mut t = TrackAnalysis {
        track_id: TrackId(1),
        duration_sec: 24.,
        sample_rate: 48000,
        tempo: TempoMap {
            global_bpm: 120.,
            meter_num: 4,
            meter_den: 4,
            beats: (0..=48).map(|i| i as f32 * 0.5).collect(),
            downbeats: (0..=12).map(|i| i as f32 * 2.).collect(),
            segments: vec![TempoSegment {
                start_beat: 0.,
                end_beat: 48.,
                bpm: 120.,
                confidence: 0.9,
            }],
            ..Default::default()
        },
        key: None,
        camelot: None,
        key_confidence: 0.,
        sections: vec![],
        bars: vec![],
        phrase_boundaries: vec![],
        mix_regions: vec![],
        moments: vec![],
        tonal_frames: vec![],
        stems: Some(StemAnalysis {
            version: 2,
            separator_sha256: String::new(),
            notes_sha256: String::new(),
            duration_sec: 24.,
            residual_rms: 0.,
            notes: vec![],
            frames: (0..480)
                .map(|i| StemFrame {
                    start_sec: i as f32 * 0.05,
                    end_sec: (i + 1) as f32 * 0.05,
                    rms: [0.1; 3],
                    band_db: [[-20.; 3]; 3],
                    onset: [0.5; 3],
                    drum_low_onset: 0.1,
                    vocal_activity: 0.,
                    note_chroma: [0.; 12],
                })
                .collect(),
        }),
        waveform_path: None,
        partial: false,
    };
    for cycle in 0..6 {
        for (i, pitch) in [60, 64, 67, 62].into_iter().enumerate() {
            let start = cycle as f32 * 4. + i as f32;
            t.stems.as_mut().unwrap().notes.push(StemNote {
                stem: StemKind::Instruments,
                start_sec: start,
                end_sec: start + 0.8,
                midi: pitch,
                confidence: 0.9,
            });
        }
    }
    t
}

#[test]
fn recurring_pitch_and_rhythm_find_cycles_not_every_note_gap() {
    let t = recording();
    let b = instrument_boundaries(&t);
    assert!(!b.is_empty());
    assert!(b
        .iter()
        .all(|b| b.kind == InstrumentBoundaryKind::Motif && b.period_sec == Some(4.)));
    assert!(b
        .iter()
        .any(|b| b.next_attack_sec == 8. && (b.release_sec - 7.8).abs() < 0.001));
    assert!(b
        .iter()
        .all(|b| b.note_similarity > 0.99 && b.contour_similarity > 0.99));
}

#[test]
fn a_phrase_can_span_several_analysis_clock_segments() {
    let mut t = recording();
    t.tempo.segments = (0..3)
        .map(|i| TempoSegment {
            start_beat: i as f32 * 16.,
            end_beat: (i + 1) as f32 * 16.,
            bpm: 120.,
            confidence: 0.9,
        })
        .collect();
    assert!(instrument_boundaries(&t)
        .iter()
        .any(|b| b.next_attack_sec == 8.));
    t.tempo.segments[1].confidence = 0.1;
    assert!(!instrument_boundaries(&t)
        .iter()
        .any(|b| b.next_attack_sec == 8.));
}

#[test]
fn different_contours_tremolo_pads_and_a_crossing_voice_do_not_become_motif_endings() {
    for variant in 0..7 {
        let mut t = recording();
        let s = t.stems.as_mut().unwrap();
        match variant {
            0 => {
                for (i, n) in s.notes.iter_mut().enumerate() {
                    n.midi = 45 + i as u8;
                }
            }
            1 => {
                for n in &mut s.notes {
                    n.midi = 60;
                }
            }
            2 => {
                s.notes = vec![StemNote {
                    stem: StemKind::Instruments,
                    start_sec: 0.,
                    end_sec: 24.,
                    midi: 60,
                    confidence: 0.9,
                }];
            }
            3 => {
                s.notes.push(StemNote {
                    stem: StemKind::Instruments,
                    start_sec: 0.,
                    end_sec: 24.,
                    midi: 80,
                    confidence: 0.9,
                });
                s.notes.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
            }
            4 => {
                for n in &mut s.notes {
                    n.confidence = 0.3;
                }
            }
            5 => t.tempo.segments[0].confidence = 0.2,
            6 => {
                s.frames.remove(100);
            }
            _ => unreachable!(),
        }
        assert!(instrument_boundaries(&t).is_empty(), "variant {variant}");
    }
}

#[test]
fn polyphonic_storage_order_is_not_melodic_contour() {
    let mut t = recording();
    let s = t.stems.as_mut().unwrap();
    for mut n in s.notes.clone() {
        n.midi += 7;
        s.notes.push(n);
    }
    s.notes.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    let before = instrument_boundaries(&t);
    t.stems.as_mut().unwrap().notes.sort_by(|a, b| {
        a.start_sec
            .total_cmp(&b.start_sec)
            .then(b.midi.cmp(&a.midi))
    });
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(instrument_boundaries(&t)).unwrap()
    );
}

#[test]
fn note_gaps_need_measured_instrument_retreat() {
    let mut t = recording();
    let s = t.stems.as_mut().unwrap();
    // An evolving, non-repeating line followed by a real rest.
    s.notes.truncate(5);
    s.notes[4].start_sec = 8.;
    s.notes[4].end_sec = 8.8;
    s.notes[4].midi = 73;
    assert!(instrument_boundaries(&t).is_empty());
    for f in &mut t.stems.as_mut().unwrap().frames {
        if f.start_sec >= 3.8 && f.end_sec <= 8. {
            f.rms[2] *= 0.1;
        }
    }
    let b = instrument_boundaries(&t);
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].kind, InstrumentBoundaryKind::Rest);
    assert!((b[0].release_sec - 3.8).abs() < 0.001 && b[0].next_attack_sec == 8.);
}

#[test]
fn gain_pitch_speed_translation_and_half_double_pulse_preserve_motifs() {
    for alias in [0.5, 1., 2.] {
        for speed in [0.8, 1., 1.2] {
            let mut t = recording();
            let offset = 2.;
            let time = |s: f32| s / speed + offset;
            t.duration_sec = time(t.duration_sec);
            t.tempo.global_bpm *= alias * speed;
            t.tempo.beats = (0..=(48. * alias) as usize)
                .map(|i| time(i as f32 * 0.5 / alias))
                .collect();
            for b in &mut t.tempo.downbeats {
                *b = time(*b);
            }
            t.tempo.segments[0].bpm *= alias * speed;
            t.tempo.segments[0].end_beat *= alias;
            let s = t.stems.as_mut().unwrap();
            s.duration_sec = t.duration_sec;
            for n in &mut s.notes {
                n.start_sec = time(n.start_sec);
                n.end_sec = time(n.end_sec);
                n.midi += 3;
            }
            for f in &mut s.frames {
                f.start_sec = time(f.start_sec);
                f.end_sec = time(f.end_sec);
                f.rms = f.rms.map(|x| x * 0.1);
            }
            let mut prefix = s.frames[0].clone();
            prefix.start_sec = 0.;
            prefix.end_sec = offset;
            prefix.rms = [0.; 3];
            s.frames.insert(0, prefix);
            let b = instrument_boundaries(&t);
            assert!(
                b.iter()
                    .any(|b| (b.next_attack_sec - time(8.)).abs() < 0.001),
                "alias {alias} speed {speed}"
            );
            assert!(b
                .iter()
                .all(|b| (b.period_sec.unwrap() - 4. / speed).abs() < 0.001));
        }
    }
}

#[test]
fn tempo_changes_and_nested_scopes_do_not_reuse_an_unconfirmed_cycle() {
    let a = recording();
    let mut b = a.clone();
    b.stems = None;
    {
        let _a = InstrumentScope::new(&a, &a);
        assert!(!instrument_boundaries(&a).is_empty());
        {
            let _b = InstrumentScope::new(&b, &b);
            assert!(instrument_boundaries(&b).is_empty());
        }
        assert!(!instrument_boundaries(&a).is_empty());
    }
    let mut t = a;
    for beat in &mut t.tempo.downbeats {
        if *beat > 8. {
            *beat = 8. + (*beat - 8.) * 0.8;
        }
    }
    assert!(!instrument_boundaries(&t)
        .iter()
        .any(|b| (b.next_attack_sec - 8.).abs() < 0.001));
}

#[test]
fn prepared_evidence_is_shared_without_sharing_worker_scopes() {
    let a = recording();
    let mut b = a.clone();
    b.stems = None;
    let expected = serde_json::to_value(instrument_boundaries(&a)).unwrap();
    let evidence = InstrumentEvidence::new(&a, &a);
    std::thread::scope(|threads| {
        for _ in 0..4 {
            threads.spawn(|| {
                let _scope = evidence.enter();
                assert_eq!(
                    serde_json::to_value(instrument_boundaries(&a)).unwrap(),
                    expected
                );
                {
                    let _nested = InstrumentEvidence::new(&b, &b).enter();
                    assert!(instrument_boundaries(&b).is_empty());
                }
                assert_eq!(
                    serde_json::to_value(instrument_boundaries(&a)).unwrap(),
                    expected
                );
            });
        }
    });
    assert!(instrument_boundaries(&b).is_empty());
}
