//! Independent PCM fixtures: labels come from the generator, never from the
//! analyzer's own bars, sections or keys. These do not replace annotated music.
use super::*;
use serde_json::json;

fn pulse(sr: u32, bpm: f32, meter: usize, offset: f32, gain: f32) -> Vec<f32> {
    let mut pcm = Vec::with_capacity(sr as usize * 48 * 2);
    for i in 0..sr * 48 {
        let t = i as f32 / sr as f32 - offset;
        let beat = (t * bpm / 60.).max(0.);
        let phase = beat.fract() * 60. / bpm;
        let accent = if beat.floor() as usize % meter == 0 {
            0.8
        } else {
            0.4
        };
        let s = if t < 0. {
            0.
        } else {
            gain * accent * (-phase * 40.).exp() * (std::f32::consts::TAU * 70. * phase).sin()
        };
        pcm.extend([s, s]);
    }
    pcm
}

fn p95_error(times: &[f32], period: f32, offset: f32) -> f32 {
    let mut errors: Vec<_> = times
        .iter()
        .filter(|t| **t > 2. && **t < 46.)
        .map(|t| {
            let beats = (*t - offset) / period;
            (beats - beats.round()).abs() * period
        })
        .collect();
    errors.sort_by(f32::total_cmp);
    errors
        .get(errors.len() * 95 / 100)
        .copied()
        .unwrap_or(f32::INFINITY)
}

fn pulse_metrics(sr: u32, bpm: f32, meter: usize, offset: f32, gain: f32) -> serde_json::Value {
    let a = analyze(TrackId(0), &pulse(sr, bpm, meter, offset, gain), sr);
    json!({"source_rate": sr, "expected_bpm": bpm, "expected_meter": meter,
        "offset": offset, "gain": gain, "bpm": a.tempo.global_bpm,
        "meter": a.tempo.meter_num,
        "max_bar_confidence": a.tempo.segments.iter().map(|s| s.confidence).fold(0., f32::max),
        "max_pulse_confidence": a.tempo.pulse_confidence.iter().copied().fold(0., f32::max),
        "beat_p95_seconds": p95_error(&a.tempo.beats, 60. / bpm, offset),
        "downbeat_p95_seconds": p95_error(&a.tempo.downbeats, 60. / bpm * meter as f32, offset)})
}

#[test]
fn known_pcm_grid_preserves_phase_under_gain_offset_and_source_rate() {
    for (sr, bpm, offset, gain) in [
        (22050, 90., 0.137, 0.25),
        (44100, 127.5, 0., 1.),
        (48000, 174., 0.217, 0.5),
    ] {
        let m = pulse_metrics(sr, bpm, 4, offset, gain);
        assert!((m["bpm"].as_f64().unwrap() - bpm as f64).abs() < 0.4, "{m}");
        assert!(m["beat_p95_seconds"].as_f64().unwrap() < 0.04, "{m}");
        assert!(m["downbeat_p95_seconds"].as_f64().unwrap() < 0.04, "{m}");
    }
}

#[test]
fn known_pcm_chroma_follows_transposition_without_using_predicted_keys_as_truth() {
    for shift in [0, 1, 5, 11] {
        let notes = [60 + shift, 64 + shift, 67 + shift];
        let mut pcm = vec![];
        for i in 0..22050 * 5 {
            let t = i as f32 / 22050.;
            let s = notes
                .iter()
                .map(|midi| {
                    let hz = 440. * 2f32.powf((*midi as f32 - 69.) / 12.);
                    0.1 * (std::f32::consts::TAU * hz * t).sin()
                })
                .sum::<f32>();
            pcm.extend([s, s]);
        }
        let a = analyze(TrackId(0), &pcm, 22050);
        let mut chroma = [0.; 12];
        for f in &a.tonal_frames {
            for (i, value) in f.chroma.iter().enumerate() {
                chroma[i] += value;
            }
        }
        let mut ranks: Vec<_> = (0..12).collect();
        ranks.sort_by(|a, b| chroma[*b].total_cmp(&chroma[*a]));
        let mut actual = ranks[..3].to_vec();
        actual.sort();
        let mut expected: Vec<_> = notes.iter().map(|n| (n % 12) as usize).collect();
        expected.sort();
        assert_eq!(actual, expected, "shift={shift} chroma={chroma:?}");
    }
}

#[test]
fn known_odd_meter_pcm_has_the_correct_meter_and_downbeat_phase() {
    for meter in [3, 5, 7] {
        for bpm in [90., 127.5, 174.] {
            let result = pulse_metrics(22050, bpm, meter, 0.137, 0.5);
            assert_eq!(result["meter"].as_u64(), Some(meter as u64), "{result}");
            assert!(
                result["max_bar_confidence"].as_f64().unwrap() >= 0.65,
                "{result}"
            );
            assert!(
                result["downbeat_p95_seconds"].as_f64().unwrap() < 0.04,
                "{result}"
            );
        }
    }
}

#[test]
#[ignore = "offline known-PCM audit: MIXLESS_ANALYSIS_VALIDATION_OUTPUT"]
fn audit_known_pcm_meter_and_transformations() {
    let mut results = vec![];
    for meter in [3, 4, 5, 7] {
        for (sr, bpm, offset, gain) in [
            (22050, 90., 0., 1.),
            (44100, 127.5, 0.137, 0.25),
            (48000, 174., 0.217, 0.5),
        ] {
            results.push(pulse_metrics(sr, bpm, meter, offset, gain));
        }
    }
    let output = std::env::var("MIXLESS_ANALYSIS_VALIDATION_OUTPUT").expect("output JSON");
    std::fs::write(output, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
}
