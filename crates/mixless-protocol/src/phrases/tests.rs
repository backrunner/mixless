use super::*;
use crate::{BarFeature, Section, TempoMap, TrackId};

fn recording(clock: f32, gap_level: f32) -> TrackAnalysis {
    let sections = [
        (0., 8., S::Intro),
        (8., 40., S::Drop),
        (40., 48., S::Break),
        (48., 80., S::Drop),
        (80., 88., S::Outro),
    ]
    .into_iter()
    .map(|(start_sec, end_sec, label)| Section {
        start_sec,
        end_sec,
        label,
    })
    .collect();
    let step = 2. / clock;
    let bars = (0..(88. / step) as usize)
        .map(|i| {
            let start = i as f32 * step;
            let gap = (40. ..48.).contains(&start);
            let level = if gap { gap_level } else { 1. };
            BarFeature {
                bar_index: i as u32,
                start_sec: start,
                end_sec: start + step,
                rms: 0.4 * level,
                crest: 2.,
                low_db: -10. + 20. * level.log10(),
                mid_db: -16.,
                high_db: -22.,
                chroma: [0.; 12],
                chord: None,
                local_key: None,
                onset_density: 20. * level,
                kick_salience: 0.9 * level,
                hat_salience: 0.5,
                vocal_presence: 0.,
                vocal_confidence: None,
                energy_slope: 0.,
                section: if gap { S::Break } else { S::Drop },
            }
        })
        .collect();
    TrackAnalysis {
        track_id: TrackId(0),
        duration_sec: 88.,
        sample_rate: 48000,
        tempo: TempoMap {
            global_bpm: 120. * clock,
            meter_num: 4,
            meter_den: 4,
            ..Default::default()
        },
        key: None,
        camelot: None,
        key_confidence: 0.,
        sections,
        bars,
        phrase_boundaries: vec![],
        mix_regions: vec![],
        moments: vec![],
        stems: None,
        waveform_path: None,
        partial: true,
    }
}

#[test]
fn suspension_is_stable_under_gain_speed_offset_and_half_double_clock() {
    for clock in [0.5, 1., 2.] {
        for speed in [0.75, 1., 1.5] {
            for gain in [0.125_f32, 0.5, 1., 2.] {
                let mut t = recording(clock, 0.25);
                let offset = 3.7;
                for b in &mut t.bars {
                    b.start_sec = b.start_sec / speed + offset;
                    b.end_sec = b.end_sec / speed + offset;
                    b.rms *= gain;
                    b.low_db += 20. * gain.log10();
                    b.onset_density *= speed;
                }
                for s in &mut t.sections {
                    s.start_sec = s.start_sec / speed + offset;
                    s.end_sec = s.end_sec / speed + offset;
                }
                t.duration_sec = t.duration_sec / speed + offset;
                t.tempo.global_bpm *= speed;
                let start = 40. / speed + offset;
                let end = 48. / speed + offset;
                assert_eq!(
                    drop_suspension(&t, start),
                    Some((start, end)),
                    "clock={clock} speed={speed} gain={gain}"
                );
                assert!(drop_suspension(&t, end).is_none());
            }
        }
    }
}

#[test]
fn a_short_section_label_without_measured_retreat_is_not_a_suspension() {
    let t = recording(1., 1.);
    assert!(drop_suspension(&t, 40.).is_none());
    let mut bass_mute = t.clone();
    for b in &mut bass_mute.bars[20..24] {
        b.low_db -= 18.;
    }
    assert!(drop_suspension(&bass_mute, 40.).is_none());
    let mut steady_drums = recording(1., 0.25);
    for b in &mut steady_drums.bars[20..24] {
        b.onset_density = 20.;
    }
    assert!(drop_suspension(&steady_drums, 40.).is_none());
}

#[test]
fn a_quiet_recovery_with_its_own_phrase_remains_an_independent_passage() {
    let mut t = recording(1., 0.25);
    t.phrase_boundaries.push(crate::PhraseBoundary {
        time_sec: 44.,
        confidence: 0.8,
        novelty: 0.4,
    });
    assert!(drop_suspension(&t, 40.).is_none());
}

#[test]
fn missing_measurements_faded_return_and_long_independent_section_do_not_certify_a_pause() {
    let mut t = recording(1., 0.25);
    t.bars.retain(|b| !(42. ..46.).contains(&b.start_sec));
    assert!(drop_suspension(&t, 40.).is_none());
    let mut t = recording(1., 0.02);
    for b in t.bars.iter_mut().filter(|b| b.start_sec >= 48.) {
        b.rms *= 0.1;
        b.low_db -= 20.;
    }
    assert!(drop_suspension(&t, 40.).is_none());
    // The same eight-second gap is a substantial independent section when
    // its neighbouring passages are only twelve seconds, not a brief breath.
    let mut t = recording(1., 0.25);
    t.sections[1].start_sec = 28.;
    t.sections[0].end_sec = 28.;
    t.sections[3].end_sec = 60.;
    t.sections[4].start_sec = 60.;
    assert!(drop_suspension(&t, 40.).is_none());
}
