use super::*;
fn drums_at(sr: u32, bpm: f32, seconds: f32, t0: f32) -> Vec<f32> {
    let mut samples = Vec::new();
    for i in 0..(sr as f32 * seconds) as usize {
        let t = t0 + i as f32 / sr as f32;
        let beat = t * bpm / 60.;
        let phase = beat.fract() * 60. / bpm;
        let accent = if beat.floor() as usize % 4 == 0 {
            0.8
        } else {
            0.4
        };
        let s = accent * (-phase * 40.).exp() * (std::f32::consts::TAU * 70. * phase).sin();
        samples.extend([s, s]);
    }
    samples
}
fn drums(sr: u32, bpm: f32, seconds: f32) -> Vec<f32> {
    drums_at(sr, bpm, seconds, 0.)
}
// Consecutive (bpm, seconds) spans sharing one continuous phase; bpm <= 0 is
// silence, so breaks and tempo changes join at exact musical boundaries.
fn parts(sr: u32, spans: &[(f32, f32)]) -> Vec<f32> {
    let mut samples = Vec::new();
    let mut t0 = 0.;
    for &(bpm, seconds) in spans {
        if bpm > 0. {
            samples.extend(drums_at(sr, bpm, seconds, t0));
        } else {
            samples.resize(samples.len() + (sr as f32 * seconds) as usize * 2, 0.);
        }
        t0 += seconds;
    }
    samples
}
#[test]
fn silence_is_unknown_and_never_supplies_a_confident_grid() {
    let a = analyze(TrackId(1), &vec![0.; 44100 * 4 * 2], 44100);
    assert_eq!(a.sample_rate, 44100);
    assert_eq!(a.key, None);
    assert_eq!(a.key_confidence, 0.);
    assert_eq!(a.tempo.segments[0].confidence, 0.);
    assert!(a.tempo.beats.is_empty());
    assert!(a.tempo.downbeats.is_empty());
    assert!(a
        .bars
        .iter()
        .all(|b| b.rms == 0. && b.section == S::Silence));
}
#[test]
fn accented_click_grid_tracks_fractional_bpm_at_source_rates() {
    for sr in [44100, 48000] {
        let a = analyze(TrackId(1), &drums(sr, 127.5, 24.), sr);
        eprintln!(
            "sr={sr} bpm={} confidence={} first={:?}",
            a.tempo.global_bpm,
            a.tempo.segments[0].confidence,
            a.tempo.beats.first()
        );
        assert!((a.tempo.global_bpm - 127.5).abs() < 0.3);
        assert!(a.tempo.beats.windows(2).all(|p| p[1] > p[0]));
        assert!(a
            .bars
            .iter()
            .all(|b| b.rms.is_finite() && b.vocal_presence.is_finite()));
        assert!(a.bars.iter().any(|b| b.kick_salience > 0.3));
    }
}
#[test]
fn analysis_finds_trailing_silence_and_sustained_foreground() {
    let sr = 22050;
    let mut samples = Vec::new();
    for i in 0..sr * 12 {
        let t = i as f32 / sr as f32;
        let s = if t < 8. {
            [220., 277.18, 329.63]
                .iter()
                .map(|hz| (std::f32::consts::TAU * hz * t).sin() * 0.1)
                .sum::<f32>()
        } else {
            0.
        };
        samples.extend([s, s]);
    }
    let a = analyze(TrackId(2), &samples, sr);
    assert!(a.bars.iter().any(|b| b.vocal_presence > 0.5));
    assert!(a.bars.last().is_some_and(|b| b.section == S::Silence));
    assert!(a.key.is_some());
}

#[test]
fn unaccented_beats_do_not_claim_reliable_downbeats() {
    let sr = 22050;
    let mut samples = drums(sr, 120., 32.);
    for (i, frame) in samples.chunks_exact_mut(2).enumerate() {
        if (i as f32 / sr as f32 * 2.).floor() as usize % 4 == 0 {
            frame[0] *= 0.5;
            frame[1] *= 0.5;
        }
    }
    let a = analyze(TrackId(1), &samples, sr);
    assert!((a.tempo.global_bpm - 120.).abs() < 0.3);
    assert!(a.tempo.segments.iter().all(|s| s.confidence < 0.65));
    assert_eq!(a.tempo.pulse_confidence.len(), a.tempo.segments.len());
    assert!(
        a.tempo.pulse_confidence.iter().any(|v| *v >= 0.65),
        "a reliable pulse does not imply bar one"
    );
    let accented = analyze(TrackId(2), &drums(sr, 120., 32.), sr);
    assert!(accented.tempo.segments.iter().any(|s| s.confidence >= 0.65));
}

#[test]
fn tempo_change_cannot_inherit_confidence_from_a_constant_region() {
    let sr = 22050;
    let mut samples = drums(sr, 120., 24.);
    samples.extend(drums(sr, 132., 24.));
    let a = analyze(TrackId(1), &samples, sr);
    assert!(a.tempo.segments.len() > 1);
    assert!(a.tempo.segments.iter().any(|s| s.confidence < 0.65));
}

#[test]
fn tempo_segments_measure_distinct_section_bpms() {
    let sr = 22050;
    let mut samples = drums(sr, 100., 24.);
    samples.extend(drums(sr, 140., 24.));
    let a = analyze(TrackId(3), &samples, sr);
    let first = a.tempo.segments.first().unwrap().bpm;
    let last = a.tempo.segments.last().unwrap().bpm;
    assert!(first < 115., "first segment BPM was {first}");
    assert!(last > 125., "last segment BPM was {last}");
    assert!(a.tempo.beats.windows(2).all(|w| w[1] > w[0]));
}

#[test]
fn structure_classifier_exposes_dj_entry_and_exit_regions() {
    let mut samples = drums(22050, 120., 64.);
    for (i, frame) in samples.chunks_exact_mut(2).enumerate() {
        let sec = i as f32 / 22050.;
        if sec < 16. || sec >= 48. {
            frame[0] *= 0.3;
            frame[1] *= 0.3;
        }
    }
    let a = analyze(TrackId(4), &samples, 22050);
    assert!(a.sections.iter().any(|s| s.label == S::Intro));
    assert!(a.sections.iter().any(|s| s.label == S::Drop));
    assert!(a.sections.iter().any(|s| s.label == S::Outro));
    assert!(a.bars.windows(2).all(|w| w[1].start_sec >= w[0].start_sec));
}

#[test]
fn antiphase_stereo_remains_audible_to_structure_detection() {
    let mut samples = drums(22050, 120., 12.);
    for frame in samples.chunks_exact_mut(2) {
        frame[1] = -frame[0];
    }
    let a = analyze(TrackId(5), &samples, 22050);
    assert!(a
        .bars
        .iter()
        .any(|b| b.rms > 0.01 && b.section != S::Silence));
    assert!(!a.tempo.beats.is_empty());
}

#[test]
fn slow_beats_are_not_blindly_doubled_into_dnb() {
    for bpm in [80., 87., 174.] {
        let a = analyze(TrackId(10), &drums(22050, bpm, 24.), 22050);
        assert!(
            (a.tempo.global_bpm - bpm).abs() < 0.4,
            "{bpm} detected as {}",
            a.tempo.global_bpm
        );
    }
}

#[test]
fn a_drumless_break_cannot_drift_a_constant_grid() {
    let sr = 22050;
    // Exactly 36 beats of silence between two 174 BPM passages: the second
    // passage re-enters on the continuing grid.
    let samples = parts(sr, &[(174., 30.), (0., 36. * 60. / 174.), (174., 30.)]);
    let a = analyze(TrackId(11), &samples, sr);
    let global = a.tempo.global_bpm;
    assert!((global - 174.).abs() < 0.4, "global {global}");
    assert!(
        a.tempo
            .segments
            .iter()
            .all(|s| (s.bpm - global).abs() < 0.01),
        "segments {:?}",
        a.tempo.segments.iter().map(|s| s.bpm).collect::<Vec<_>>()
    );
    let period = 60. / global;
    let phase = a.tempo.beats[0];
    for (i, &beat) in a.tempo.beats.iter().enumerate() {
        assert!(
            (beat - (phase + i as f32 * period)).abs() < 0.005,
            "beat {i} at {beat}"
        );
    }
    // A break measures no local pulse, but a constant grid reports the
    // globally validated one instead of a false zero.
    assert!(
        a.tempo.pulse_confidence.iter().all(|v| *v >= 0.65),
        "pulse {:?}",
        a.tempo.pulse_confidence
    );
}

#[test]
fn a_single_wrong_window_cannot_phase_shift_the_grid() {
    let sr = 22050;
    // One 16-beat window of 165 BPM inside a constant 174 BPM track.
    let samples = parts(sr, &[(174., 16.55), (165., 5.52), (174., 33.07)]);
    let a = analyze(TrackId(12), &samples, sr);
    eprintln!(
        "global={} segments={:?} pulse={:?}",
        a.tempo.global_bpm,
        a.tempo
            .segments
            .iter()
            .map(|s| (s.bpm * 10.).round() / 10.)
            .collect::<Vec<_>>(),
        a.tempo.pulse_confidence
    );
    let global = a.tempo.global_bpm;
    assert!((global - 174.).abs() < 0.5, "global {global}");
    assert!(
        a.tempo
            .segments
            .iter()
            .all(|s| (s.bpm - global).abs() < 0.01),
        "segments {:?}",
        a.tempo.segments.iter().map(|s| s.bpm).collect::<Vec<_>>()
    );
    let period = 60. / global;
    let phase = a.tempo.beats[0];
    for (i, &beat) in a.tempo.beats.iter().enumerate() {
        assert!(
            (beat - (phase + i as f32 * period)).abs() < 0.005,
            "beat {i} at {beat}"
        );
    }
}

#[test]
fn a_sustained_tempo_change_survives_the_outlier_filter() {
    let sr = 22050;
    let samples = parts(sr, &[(174., 40.), (150., 24.)]);
    let a = analyze(TrackId(13), &samples, sr);
    eprintln!(
        "global={} segments={:?}",
        a.tempo.global_bpm,
        a.tempo
            .segments
            .iter()
            .map(|s| (s.bpm * 10.).round() / 10.)
            .collect::<Vec<_>>()
    );
    let first = a.tempo.segments.first().unwrap().bpm;
    let last = a.tempo.segments.last().unwrap().bpm;
    assert!((first - 174.).abs() < 1.5, "first {first}");
    assert!((last - 150.).abs() < 2.5, "last {last}");
}
