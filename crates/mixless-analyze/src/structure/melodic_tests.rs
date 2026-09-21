use super::*;
use mixless_protocol::{CueKind, MixRegionKind, TrackId};

#[test]
fn measured_melodic_pauses_do_not_become_exit_cues() {
    // Bar measurements only. No audio, source paths, stems or library IDs.
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/melodic.json")).unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let mut t = crate::Analyzer::new().to_track_analysis(
            TrackId(1),
            &crate::QuickAnalysis {
                bpm: fixture["bpm"].as_f64().unwrap() as f32,
                key: "Unknown".into(),
                camelot: "Unknown".into(),
                confidence: 1.,
            },
            fixture["duration_sec"].as_f64().unwrap() as f32,
        );
        t.tempo.downbeats = serde_json::from_value(fixture["downbeats"].clone()).unwrap();
        for (i, row) in fixture["bars"].as_array().unwrap().iter().enumerate() {
            let f = |k: usize| row[k].as_f64().unwrap() as f32;
            t.bars.push(BarFeature {
                bar_index: i as u32,
                start_sec: f(0),
                end_sec: f(1),
                rms: f(2),
                low_db: f(3),
                mid_db: f(4),
                high_db: f(5),
                onset_density: f(6),
                kick_salience: f(7),
                vocal_presence: f(8),
                vocal_confidence: row[9].as_f64().map(|v| v as f32),
                chroma: serde_json::from_value(row[10].clone()).unwrap(),
                crest: 2.,
                chord: None,
                local_key: None,
                hat_salience: 0.,
                energy_slope: 0.,
                section: S::Unknown,
            });
        }
        crate::Analyzer::refresh_structure(&mut t);
        let cues = crate::automatic_cues(&t);
        let out = cues.iter().find(|c| c.kind == CueKind::Out).unwrap().frame as f32
            / t.sample_rate as f32;
        if fixture["title"] == "Day Cycle" {
            assert!((89.5..90.).contains(&out), "out={out}, {:?}", t.sections);
            assert!(
                t.mix_regions
                    .iter()
                    .all(|r| r.kind != MixRegionKind::Out || !(60.5..67.4).contains(&r.anchor_sec)),
                "{:?}",
                t.mix_regions
            );
            assert!(
                t.sections.iter().any(|s| s.start_sec <= 62.
                    && s.end_sec >= 66.
                    && matches!(s.label, S::Break | S::Breakdown)),
                "{:?}",
                t.sections
            );
        } else {
            let entry = cues.iter().find(|c| c.kind == CueKind::In).unwrap().frame as f32
                / t.sample_rate as f32;
            assert!(
                (1.5..1.7).contains(&entry),
                "entry in leading silence: {entry}"
            );
            assert!((142.3..142.7).contains(&out), "out={out}, {:?}", t.sections);
            for end in [89.84, 176.55] {
                assert!(
                    t.sections.iter().any(|s| s.label == S::BuildUp
                        && (s.end_sec - end).abs() < 0.1
                        && mixless_protocol::has_buildup(&t.bars, s.start_sec, s.end_sec)),
                    "no evidenced build to {end}: {:?}",
                    t.sections
                );
            }
        }
    }
}

#[test]
fn silent_pre_drop_breath_stays_in_the_build() {
    let mut bars = super::tests::bars(48);
    for (i, b) in bars[16..24].iter_mut().enumerate() {
        b.rms = 0.08;
        b.low_db = -34.;
        b.kick_salience = 0.2;
        b.onset_density = 2. + i as f32 * 2.;
        b.high_db = -28. + i as f32;
    }
    bars[23].rms = 0.;
    bars[23].onset_density = 0.;
    bars[23].low_db = -120.;
    bars[23].high_db = -120.;
    let (sections, _) = detect(
        &mut bars,
        &(0..=48).map(|i| i as f32 * 2.).collect::<Vec<_>>(),
    );
    assert!(
        sections
            .iter()
            .any(|s| s.label == S::BuildUp && s.start_sec <= 40. && s.end_sec == 48.),
        "{sections:?}"
    );
    assert!(!sections.iter().any(|s| s.label == S::Silence));
}
