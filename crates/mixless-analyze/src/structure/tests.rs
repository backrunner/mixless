use super::*;

#[test]
fn imported_dnb_measurements_keep_drop_break_and_build_separate() {
    // Numeric bar descriptors only, no source audio or local paths. Checkpoints
    // cover sustained regions, not the detector's exact boundary timestamps.
    let tracks: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/dnb.json")).unwrap();
    for track in tracks.as_array().unwrap() {
        let mut measured = bars(track["bars"].as_array().unwrap().len());
        for (b, row) in measured.iter_mut().zip(track["bars"].as_array().unwrap()) {
            let value = |i: usize| row[i].as_f64().unwrap() as f32;
            b.start_sec = value(0);
            b.end_sec = value(1);
            b.rms = value(2);
            b.low_db = value(3);
            b.mid_db = value(4);
            b.high_db = value(5);
            b.onset_density = value(6);
            b.kick_salience = value(7);
            b.vocal_presence = value(8);
            b.vocal_confidence = row[9].as_f64().map(|x| x as f32);
            b.chroma = std::array::from_fn(|i| row[10][i].as_f64().unwrap() as f32);
        }
        let downbeats = track["downbeats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect::<Vec<_>>();
        let (sections, _) = detect(&mut measured, &downbeats);
        for check in track["checks"].as_array().unwrap() {
            let label: S = serde_json::from_value(check[2].clone()).unwrap();
            for sec in check[0].as_u64().unwrap()..check[1].as_u64().unwrap() {
                let section = sections
                    .iter()
                    .find(|s| sec as f32 >= s.start_sec && (sec as f32) < s.end_sec)
                    .unwrap();
                assert_eq!(
                    section.label, label,
                    "{} at {sec}s: {sections:?}",
                    track["title"]
                );
            }
        }
    }
}

#[test]
fn compressed_dnb_break_uses_sustained_bass_retreat_despite_saturated_kicks() {
    let mut bars = bars(96);
    for b in &mut bars {
        b.rms = 0.5;
        b.low_db = -7.;
        b.onset_density = 60.;
    }
    for b in &mut bars[32..56] {
        b.rms = 0.28;
        b.low_db = -16.;
        b.onset_density = 18.;
        b.kick_salience = 1.;
    }
    for b in &mut bars[48..56] {
        b.rms = 0.36;
        b.onset_density = 40.;
    }
    // A fill inside the first drop must not expose a premature mixing exit.
    bars[15].rms = 0.2;
    bars[15].low_db = -24.;
    let (sections, _) = detect(
        &mut bars,
        &(0..=96).map(|i| i as f32 * 2.).collect::<Vec<_>>(),
    );
    assert!(
        sections
            .iter()
            .filter(|s| s.start_sec < 64.)
            .all(|s| s.label == S::Drop),
        "{sections:?}"
    );
    assert!(
        bars[32..48]
            .iter()
            .all(|b| matches!(b.section, S::Break | S::Breakdown)),
        "{sections:?}"
    );
    assert!(
        bars[48..56].iter().all(|b| b.section == S::BuildUp),
        "{sections:?}"
    );
    assert!(
        bars[56..].iter().all(|b| b.section == S::Drop),
        "{sections:?}"
    );
}
fn bars(n: usize) -> Vec<BarFeature> {
    (0..n)
        .map(|i| BarFeature {
            bar_index: i as u32,
            start_sec: i as f32 * 2.,
            end_sec: (i + 1) as f32 * 2.,
            rms: 0.2,
            crest: 2.,
            low_db: -16.,
            mid_db: -20.,
            high_db: -25.,
            chroma: [0.; 12],
            chord: None,
            local_key: None,
            onset_density: 2.,
            kick_salience: 0.8,
            hat_salience: 0.3,
            vocal_presence: 0.1,
            vocal_confidence: None,
            energy_slope: 0.,
            section: S::Unknown,
        })
        .collect()
}
#[test]
fn same_loudness_timbre_change_is_a_boundary() {
    let mut bars = bars(48);
    for bar in &mut bars[16..32] {
        bar.low_db = -28.;
        bar.mid_db = -14.;
        bar.chroma[4] = 1.;
    }
    let downbeats: Vec<_> = (0..=48).map(|i| i as f32 * 2.).collect();
    let (sections, phrases) = detect(&mut bars, &downbeats);
    for sec in [32., 64.] {
        assert!(
            sections.iter().any(|s| (s.start_sec - sec).abs() < 0.01),
            "{sections:?}"
        );
        assert!(phrases
            .iter()
            .any(|p| (p.time_sec - sec).abs() < 0.01 && p.novelty > 0.2));
    }
}
#[test]
fn an_isolated_fill_does_not_split_a_section_or_invent_intro_outro() {
    let mut bars = bars(64);
    bars[31].rms = 0.8;
    bars[31].high_db = -8.;
    bars[31].onset_density = 9.;
    let (sections, phrases) = detect(
        &mut bars,
        &(0..=64).map(|i| i as f32 * 2.).collect::<Vec<_>>(),
    );
    assert_eq!(sections.len(), 1, "{sections:?}");
    assert_eq!(sections[0].label, S::Drop);
    assert!(phrases.iter().all(|p| p.novelty < 0.16));
}
#[test]
fn phrases_follow_a_pickup_and_non_multiple_of_eight_section_start() {
    let mut bars = bars(50);
    for b in &mut bars {
        b.start_sec += 0.37;
        b.end_sec += 0.37;
    }
    for b in &mut bars[11..] {
        b.low_db = -28.;
        b.mid_db = -14.;
        b.chroma[4] = 1.;
    }
    bars.insert(
        0,
        BarFeature {
            start_sec: 0.,
            end_sec: 0.37,
            rms: 0.,
            ..bars[0].clone()
        },
    );
    let downbeats: Vec<_> = (0..=50).map(|i| 0.37 + i as f32 * 2.).collect();
    let (_, phrases) = detect(&mut bars, &downbeats);
    for n in [11, 19, 27] {
        assert!(
            phrases
                .iter()
                .any(|p| (p.time_sec - (0.37 + n as f32 * 2.)).abs() < 0.01),
            "{phrases:?}"
        );
    }
}
#[test]
fn silence_and_unknown_grid_do_not_create_confident_phrases() {
    let mut bars = bars(24);
    for b in &mut bars {
        b.rms = 0.;
    }
    let (sections, phrases) = detect(&mut bars, &[]);
    assert!(phrases.is_empty());
    assert!(sections.iter().all(|s| s.label == S::Silence));
}

#[test]
fn human_voice_evidence_overrides_plosive_kick_proxy_without_clearing_risk() {
    let mut bars = bars(24);
    for b in &mut bars {
        b.vocal_confidence = Some(0.95);
        b.vocal_presence = 0.95;
    }
    let (sections, _) = detect(&mut bars, &[]);
    assert_eq!(sections[0].label, S::Verse);
    assert!(bars.iter().all(|b| b.vocal_presence >= 0.95));
}

#[test]
fn a_flat_break_before_a_drop_is_not_a_buildup() {
    let mut bars = bars(64);
    for b in &mut bars[24..32] {
        b.rms = 0.08;
        b.low_db = -32.;
        b.kick_salience = 0.2;
        b.onset_density = 0.5;
    }
    let (sections, _) = detect(
        &mut bars,
        &(0..=64).map(|i| i as f32 * 2.).collect::<Vec<_>>(),
    );
    assert!(
        bars[24..32]
            .iter()
            .all(|b| matches!(b.section, S::Break | S::Breakdown)),
        "{sections:?}"
    );
}

#[test]
fn snare_roll_can_build_tension_without_getting_louder() {
    let mut bars = bars(48);
    for (i, b) in bars[16..24].iter_mut().enumerate() {
        b.rms = 0.08;
        b.low_db = -34.;
        b.kick_salience = 0.2;
        b.onset_density = 2. + i as f32 * 2.;
        b.high_db = -28. + i as f32;
    }
    assert!(mixless_protocol::has_buildup(&bars, 32., 48.));
    let (sections, _) = detect(
        &mut bars,
        &(0..=48).map(|i| i as f32 * 2.).collect::<Vec<_>>(),
    );
    assert!(
        sections
            .iter()
            .any(|s| s.label == S::BuildUp && (s.end_sec - 48.).abs() < 0.01),
        "{sections:?}"
    );
}
