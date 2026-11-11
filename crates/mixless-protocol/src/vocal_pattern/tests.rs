use super::*;
use crate::{StemAnalysis, TempoMap, TrackId};

fn recording() -> TrackAnalysis {
    let frames = (0..640)
        .map(|i| {
            let sec = (i as f32 + 0.5) * 0.025;
            let phase = sec.rem_euclid(0.25);
            let rms = if phase < 0.15 { 0.1 } else { 0.005 };
            let color = if sec.rem_euclid(0.5) < 0.25 {
                [0.1_f32, 0.7, 0.2]
            } else {
                [0.1, 0.3, 0.6]
            };
            StemFrame {
                start_sec: i as f32 * 0.025,
                end_sec: (i + 1) as f32 * 0.025,
                rms: [rms, 0.1, 0.1],
                band_db: [
                    color.map(|p| 10. * (p * rms * rms).log10()),
                    [-20.; 3],
                    [-20.; 3],
                ],
                onset: [0.; 3],
                drum_low_onset: 0.,
                vocal_activity: if phase < 0.15 { 0.9 } else { 0. },
                note_chroma: [0.; 12],
            }
        })
        .collect();
    let notes = (0..64)
        .map(|i| StemNote {
            stem: StemKind::Vocals,
            start_sec: i as f32 * 0.25,
            end_sec: i as f32 * 0.25 + 0.15,
            midi: if i % 2 == 0 { 60 } else { 64 },
            confidence: 0.9,
        })
        .collect();
    TrackAnalysis {
        track_id: TrackId(0),
        duration_sec: 16.,
        sample_rate: 48000,
        tempo: TempoMap {
            global_bpm: 120.,
            meter_num: 4,
            meter_den: 4,
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
            duration_sec: 16.,
            residual_rms: 0.,
            frames,
            notes,
        }),
        waveform_path: None,
        partial: false,
    }
}

#[test]
fn repeating_articulated_samples_survive_gain_offset_speed_and_equivalent_grids() {
    for gain in [0.01_f32, 1., 4.] {
        for speed in [0.8, 1., 1.2] {
            for clock in [0.5, 1., 2.] {
                for offset in [0., 3.7] {
                    let mut t = recording();
                    t.tempo.global_bpm *= speed * clock;
                    t.duration_sec = t.duration_sec / speed + offset;
                    let s = t.stems.as_mut().unwrap();
                    s.duration_sec = t.duration_sec;
                    for f in &mut s.frames {
                        f.start_sec = f.start_sec / speed + offset;
                        f.end_sec = f.end_sec / speed + offset;
                        for v in &mut f.rms {
                            *v *= gain;
                        }
                        for v in f.band_db.iter_mut().flatten() {
                            *v += 20. * gain.log10();
                        }
                    }
                    for n in &mut s.notes {
                        n.start_sec = n.start_sec / speed + offset;
                        n.end_sec = n.end_sec / speed + offset;
                    }
                    assert!(
                        vocal_pattern(&t, 8. / speed + offset).is_some(),
                        "{gain} {speed} {clock} {offset}"
                    );
                }
            }
        }
    }
}

#[test]
fn short_note_density_regular_syllables_and_a_held_vowel_do_not_prove_chops() {
    let mut t = recording();
    for f in &mut t.stems.as_mut().unwrap().frames {
        f.rms[0] = 0.1;
        f.vocal_activity = 1.;
    }
    assert!(
        vocal_pattern(&t, 8.).is_none(),
        "a held vowel fragmented by transcription"
    );
    let mut t = recording();
    for (i, n) in t.stems.as_mut().unwrap().notes.iter_mut().enumerate() {
        n.midi = 48 + ((i * i + i / 3) % 23) as u8;
    }
    assert!(
        vocal_pattern(&t, 8.).is_none(),
        "regular timing with a progressing melody"
    );
    let mut t = recording();
    // Identical note timing/pitch, changing syllable spectra: repeated beats
    // are insufficient evidence of repeated vocal material.
    for f in &mut t.stems.as_mut().unwrap().frames {
        let syllable = (f.start_sec / 0.25).floor() as usize;
        let color = match (syllable * syllable + syllable / 3) % 3 {
            0 => [0.90_f32, 0.05, 0.05],
            1 => [0.05, 0.90, 0.05],
            _ => [0.05, 0.05, 0.90],
        };
        f.band_db[0] = color.map(|p| 10. * (p * f.rms[0].powi(2)).log10());
    }
    assert!(vocal_pattern(&t, 8.).is_none(), "changing phonetic shape");
}

#[test]
fn local_sung_overlay_missing_evidence_and_incomplete_repeats_stay_protected() {
    let mut t = recording();
    t.stems.as_mut().unwrap().notes.push(StemNote {
        stem: StemKind::Vocals,
        start_sec: 6.,
        end_sec: 9.,
        midi: 67,
        confidence: 0.9,
    });
    t.stems
        .as_mut()
        .unwrap()
        .notes
        .sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    assert!(
        vocal_pattern(&t, 8.).is_none(),
        "a singer on top of a loop keeps protection"
    );
    let mut t = recording();
    t.stems.as_mut().unwrap().notes.clear();
    assert!(vocal_pattern(&t, 8.).is_none());
    let mut t = recording();
    for f in &mut t.stems.as_mut().unwrap().frames {
        f.rms[0] *= 0.001;
    }
    assert!(vocal_pattern(&t, 8.).is_none(), "separator leakage");
    let mut t = recording();
    t.stems
        .as_mut()
        .unwrap()
        .frames
        .retain(|f| f.start_sec < 7.5 || f.start_sec > 7.8);
    assert!(vocal_pattern(&t, 8.).is_none(), "missing frame coverage");
    let t = recording();
    assert!(vocal_pattern(&t, 0.5).is_none());
    let mut t = recording();
    t.stems = None;
    assert!(vocal_pattern(&t, 8.).is_none());
}

#[test]
fn changing_tempo_and_invalid_clock_do_not_invent_repetition() {
    let mut t = recording();
    t.tempo.beats = (0..40)
        .map(|i| {
            if i < 16 {
                i as f32 * 0.5
            } else {
                8. + (i - 16) as f32 * 0.65
            }
        })
        .collect();
    assert!(vocal_pattern(&t, 8.).is_none());
    for bpm in [0., f32::NAN, f32::INFINITY] {
        let mut t = recording();
        t.tempo.global_bpm = bpm;
        assert!(vocal_pattern(&t, 8.).is_none());
    }
}

#[test]
fn a_repeating_neighborhood_cannot_hide_a_new_word_at_the_edit() {
    let mut t = recording();
    t.stems.as_mut().unwrap().notes[32].midi = 67;
    assert!(vocal_pattern(&t, 8.05).is_none());
    let mut t = recording();
    for f in &mut t.stems.as_mut().unwrap().frames {
        if (8. ..8.15).contains(&f.start_sec) {
            f.band_db[0] = [0.9_f32, 0.05, 0.05].map(|p| 10. * (p * f.rms[0].powi(2)).log10());
        }
    }
    assert!(vocal_pattern(&t, 8.05).is_none());
}
