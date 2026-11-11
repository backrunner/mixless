use super::*;
use crate::{PhraseBoundary, StemAnalysis, StemFrame, TempoSegment};

pub(super) fn recording(clock: f32) -> TrackAnalysis {
    let mut t = crate::phrases::tests::recording(clock, 1.);
    t.sections[2].label = S::Drop;
    t.tempo.beats = (0..=(176. * clock) as usize)
        .map(|i| i as f32 * 0.5 / clock)
        .collect();
    t.tempo.downbeats = t.tempo.beats.iter().step_by(4).copied().collect();
    t.tempo.segments = vec![TempoSegment {
        start_beat: 0.,
        end_beat: 176. * clock,
        bpm: 120. * clock,
        confidence: 0.9,
    }];
    t.phrase_boundaries = (0..=5)
        .map(|i| PhraseBoundary {
            time_sec: i as f32 * 16.,
            confidence: 0.8,
            novelty: 0.,
        })
        .collect();
    t.stems = Some(StemAnalysis {
        version: 2,
        separator_sha256: String::new(),
        notes_sha256: String::new(),
        duration_sec: 88.,
        residual_rms: 0.,
        notes: vec![],
        frames: (0..1760)
            .map(|i| {
                let start = i as f32 * 0.05;
                let gap = (40. ..48.).contains(&start);
                StemFrame {
                    start_sec: start,
                    end_sec: start + 0.05,
                    // Strong vocals remain: this is NOT a voice release.
                    rms: [0.25, if gap { 0.12 } else { 0.2 }, 0.3],
                    onset: [0., if gap { 0.12 } else { 0.2 }, 0.],
                    band_db: [[-20.; 3]; 3],
                    drum_low_onset: 0.2,
                    vocal_activity: 1.,
                    note_chroma: [0.; 12],
                }
            })
            .collect(),
    });
    t
}

#[test]
fn limited_mix_exposes_a_sustained_relative_recovery_without_releasing_the_voice() {
    let t = recording(1.);
    assert_eq!(relative_recoveries(&t), [(40., 48.)]);
    assert_eq!(crate::peak_ranges(&t), [(8., 40.), (48., 80.)]);
    assert!(t
        .stems
        .as_ref()
        .unwrap()
        .frames
        .iter()
        .all(|f| f.vocal_activity == 1.));
    assert!(crate::drop_suspension(&t, 40.).is_none());
}

#[test]
fn prepared_recoveries_restore_each_workers_nested_evidence() {
    let a = recording(1.);
    let mut b = a.clone();
    b.stems = None;
    let evidence = PeakEvidence::new(&a, &a);
    std::thread::scope(|threads| {
        for _ in 0..4 {
            threads.spawn(|| {
                let _scope = evidence.enter();
                assert_eq!(relative_recoveries(&a), [(40., 48.)]);
                {
                    let _nested = PeakEvidence::new(&b, &b).enter();
                    assert!(relative_recoveries(&b).is_empty());
                }
                assert_eq!(relative_recoveries(&a), [(40., 48.)]);
            });
        }
    });
    assert!(relative_recoveries(&b).is_empty());
}

#[test]
fn relative_recovery_survives_gain_speed_offset_and_clock_aliases() {
    for clock in [0.5, 1., 2.] {
        for speed in [0.75, 1., 1.5] {
            for gain in [0.1, 1., 3.] {
                let mut t = recording(clock);
                let offset = 3.7;
                let shift = |x: f32| x / speed + offset;
                for b in &mut t.bars {
                    b.start_sec = shift(b.start_sec);
                    b.end_sec = shift(b.end_sec);
                    b.rms *= gain;
                }
                for s in &mut t.sections {
                    s.start_sec = shift(s.start_sec);
                    s.end_sec = shift(s.end_sec);
                }
                for p in &mut t.phrase_boundaries {
                    p.time_sec = shift(p.time_sec);
                }
                for b in &mut t.tempo.beats {
                    *b = shift(*b);
                }
                for b in &mut t.tempo.downbeats {
                    *b = shift(*b);
                }
                for s in &mut t.tempo.segments {
                    s.bpm *= speed;
                }
                t.tempo.global_bpm *= speed;
                t.duration_sec = shift(t.duration_sec);
                for f in &mut t.stems.as_mut().unwrap().frames {
                    f.start_sec = shift(f.start_sec);
                    f.end_sec = shift(f.end_sec);
                    f.rms = f.rms.map(|v| v * gain);
                }
                assert_eq!(
                    relative_recoveries(&t),
                    [(shift(40.), shift(48.))],
                    "clock={clock} speed={speed} gain={gain}"
                );
            }
        }
    }
}

#[test]
fn fills_bass_mutes_fader_changes_missing_evidence_and_risers_stay_protected() {
    for variant in 0..12 {
        let mut t = recording(1.);
        match variant {
            0 => t.stems = None,
            1 => t.tempo.segments[0].confidence = 0.2,
            2 => t
                .stems
                .as_mut()
                .unwrap()
                .frames
                .retain(|f| !(42. ..44.).contains(&f.start_sec)),
            3 => {
                // Two-bar drum fill is not an independent recovery.
                for f in &mut t.stems.as_mut().unwrap().frames {
                    if (44. ..48.).contains(&f.start_sec) {
                        f.rms[1] = 0.2;
                        f.onset[1] = 0.2;
                    }
                }
            }
            4 => {
                // A quieter kit with unchanged attacks is not arrangement evidence.
                for f in &mut t.stems.as_mut().unwrap().frames {
                    f.onset[1] = 0.2;
                }
            }
            5 => {
                // Bass-only mute, steady kit.
                for f in &mut t.stems.as_mut().unwrap().frames {
                    f.rms[1] = 0.2;
                }
                for b in &mut t.bars[20..24] {
                    b.low_db -= 18.;
                }
            }
            6 => {
                // Entire mix fades, no sustained tonal bed.
                for f in &mut t.stems.as_mut().unwrap().frames[800..960] {
                    f.rms[2] *= 0.1;
                }
            }
            7 => {
                // No supported return.
                for f in &mut t.stems.as_mut().unwrap().frames[960..] {
                    f.rms[1] = 0.12;
                    f.onset[1] = 0.12;
                }
            }
            8 => {
                for (i, b) in t.bars[20..24].iter_mut().enumerate() {
                    b.rms = 0.1 * 2_f32.powi(i as i32);
                }
            }
            9 => t.stems.as_mut().unwrap().frames[810].rms[1] = f32::NAN,
            10 => {
                // Local variability dwarfs the apparent valley.
                for f in &mut t.stems.as_mut().unwrap().frames {
                    if !(40. ..48.).contains(&f.start_sec) {
                        f.rms[1] = if (f.start_sec / 2.) as usize % 2 == 0 {
                            0.1
                        } else {
                            0.4
                        };
                    }
                }
            }
            11 => {
                t.bars.remove(22);
            }
            _ => unreachable!(),
        }
        assert!(relative_recoveries(&t).is_empty(), "variant={variant}");
    }
}

#[test]
fn nested_scopes_and_edited_analyses_cannot_reuse_stale_evidence() {
    let mut a = recording(1.);
    let mut b = a.clone();
    b.stems = None;
    {
        let _a = PeakScope::new(&a, &a);
        assert_eq!(relative_recoveries(&a), [(40., 48.)]);
        {
            let _b = PeakScope::new(&b, &b);
            assert!(relative_recoveries(&b).is_empty());
        }
        assert_eq!(relative_recoveries(&a), [(40., 48.)]);
    }
    a.stems = None;
    assert!(relative_recoveries(&a).is_empty());
}

#[test]
fn local_context_survives_a_tempo_change_in_the_return() {
    let mut t = recording(1.);
    let warp = |s: f32| if s <= 48. { s } else { 48. + (s - 48.) / 1.2 };
    for b in &mut t.bars {
        b.start_sec = warp(b.start_sec);
        b.end_sec = warp(b.end_sec);
    }
    for s in &mut t.sections {
        s.start_sec = warp(s.start_sec);
        s.end_sec = warp(s.end_sec);
    }
    for p in &mut t.phrase_boundaries {
        p.time_sec = warp(p.time_sec);
    }
    for b in &mut t.tempo.beats {
        *b = warp(*b);
    }
    for b in &mut t.tempo.downbeats {
        *b = warp(*b);
    }
    t.tempo.segments[0].end_beat = 96.;
    t.tempo.segments.push(TempoSegment {
        start_beat: 96.,
        end_beat: 176.,
        bpm: 144.,
        confidence: 0.9,
    });
    for f in &mut t.stems.as_mut().unwrap().frames {
        f.start_sec = warp(f.start_sec);
        f.end_sec = warp(f.end_sec);
    }
    t.duration_sec = warp(t.duration_sec);
    assert_eq!(relative_recoveries(&t), [(40., 48.)]);
}

#[test]
fn a_three_beat_meter_uses_its_own_bar_duration() {
    let mut t = recording(1.);
    t.phrase_boundaries.clear();
    t.tempo.meter_num = 3;
    t.tempo.global_bpm = 90.;
    t.tempo.beats = (0..=132).map(|i| i as f32 * 2. / 3.).collect();
    t.tempo.segments[0].bpm = 90.;
    t.tempo.segments[0].end_beat = 132.;
    assert_eq!(relative_recoveries(&t), [(40., 48.)]);
}
