use super::*;
fn drums(sr: u32, bpm: f32, seconds: f32) -> Vec<f32> {
    let mut samples = Vec::new();
    for i in 0..(sr as f32 * seconds) as usize {
        let t = i as f32 / sr as f32;
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
