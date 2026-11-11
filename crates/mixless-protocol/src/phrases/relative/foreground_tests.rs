use super::*;

fn recording(clock: f32) -> TrackAnalysis {
    let mut t = super::tests::recording(clock);
    for b in &mut t.bars {
        b.rms = if (40. ..48.).contains(&b.start_sec) {
            0.28
        } else {
            0.5
        };
    }
    for f in &mut t.stems.as_mut().unwrap().frames {
        let gap = (40. ..48.).contains(&f.start_sec);
        f.rms = if gap {
            [0.04, 0.2, 0.12]
        } else {
            [0.15, 0.2, 0.3]
        };
        f.onset[1] = 0.2;
        f.band_db[2] = if gap { [-28.; 3] } else { [-20.; 3] };
    }
    t
}

#[test]
fn continuing_drums_can_carry_a_foreground_valley() {
    let t = recording(1.);
    assert_eq!(relative_recoveries(&t), [(40., 48.)]);
    assert_eq!(crate::peak_ranges(&t), [(8., 40.), (48., 80.)]);
    assert!(t
        .stems
        .unwrap()
        .frames
        .iter()
        .all(|f| f.rms[1] == 0.2 && f.onset[1] == 0.2));
}

#[test]
fn bass_mute_voice_rest_mix_dip_and_short_fill_cannot_alone_release_a_peak() {
    for variant in 0..8 {
        let mut t = recording(1.);
        for b in &mut t.bars {
            if (40. ..48.).contains(&b.start_sec) && variant == 0 {
                b.rms = 0.5;
            }
        }
        for f in &mut t.stems.as_mut().unwrap().frames {
            let gap = (40. ..48.).contains(&f.start_sec);
            match variant {
                1 if gap => f.band_db[2][1..].fill(-20.), // bass-only retreat
                2 if gap => f.rms[2] = 0.3,               // voice rest, bed still foreground
                3 if gap => {
                    f.rms[0] = 0.15;
                    f.rms[2] = 0.3;
                } // mix fader dip
                4 if f.start_sec >= 44. => {
                    f.rms[0] = 0.15;
                    f.rms[2] = 0.3;
                }
                5 if f.start_sec >= 48. => {
                    f.rms[0] = 0.04;
                    f.rms[2] = 0.12;
                }
                6 if gap => f.onset[1] = 0., // no continuing rhythmic bed
                7 if gap => f.band_db[2][1] = f32::NAN,
                _ => {}
            }
        }
        assert!(relative_recoveries(&t).is_empty(), "variant={variant}");
    }
}

#[test]
fn foreground_evidence_uses_relative_levels_and_the_source_phrase_clock() {
    for clock in [0.5, 1., 2.] {
        for speed in [0.75, 1., 1.5] {
            for gain in [0.1f32, 1., 3.] {
                let mut t = recording(clock);
                let shift = |sec: f32| sec / speed + 7.;
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
                    for db in f.band_db.iter_mut().flatten() {
                        *db += 20. * gain.log10();
                    }
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
