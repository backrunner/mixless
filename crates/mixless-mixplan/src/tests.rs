use super::*;
#[path = "catalog_tests.rs"]
mod catalog_tests;
#[path = "choreography_regressions.rs"]
mod choreography_regressions;
#[path = "continuity_regressions.rs"]
mod continuity_regressions;
#[path = "recovery_regressions.rs"]
mod recovery_regressions;
#[path = "smooth_regressions.rs"]
mod smooth_regressions;
#[path = "smooth_tests.rs"]
mod smooth_tests;
#[path = "transition_regressions.rs"]
mod transition_regressions;
#[path = "window_regressions.rs"]
mod window_regressions;
use mixless_protocol::{BarFeature, CueKind, Section, SectionLabel as S, TempoMap, TempoSegment};
pub(crate) fn track(
    id: i64,
    bpm: f32,
    cam: &str,
    label: S,
    bars: u32,
    kick: f32,
    rms: f32,
) -> TrackAnalysis {
    let seconds = bars as f32 * 240.0 / bpm;
    TrackAnalysis {
        track_id: mixless_protocol::TrackId(id),
        duration_sec: seconds,
        sample_rate: 48000,
        tempo: TempoMap {
            pulse_confidence: vec![],
            global_bpm: bpm,
            meter_num: 4,
            meter_den: 4,
            segments: vec![TempoSegment {
                start_beat: 0.,
                end_beat: bars as f32 * 4.,
                bpm,
                confidence: 1.,
            }],
            beats: (0..=bars * 4).map(|i| i as f32 * 60.0 / bpm).collect(),
            downbeats: (0..=bars).map(|i| i as f32 * 240.0 / bpm).collect(),
        },
        key: None,
        camelot: Some(cam.into()),
        key_confidence: 1.,
        sections: vec![Section {
            start_sec: 0.,
            end_sec: seconds,
            label,
        }],
        bars: (0..bars)
            .map(|i| BarFeature {
                bar_index: i,
                start_sec: i as f32 * 240.0 / bpm,
                end_sec: (i + 1) as f32 * 240.0 / bpm,
                rms,
                crest: 2.,
                low_db: -6.,
                mid_db: -12.,
                high_db: -18.,
                chroma: [0.; 12],
                chord: None,
                local_key: None,
                onset_density: 1.,
                kick_salience: kick,
                hat_salience: 0.5,
                vocal_presence: 0.1,
                vocal_confidence: None,
                energy_slope: 0.,
                section: label,
            })
            .collect(),
        phrase_boundaries: vec![],
        mix_regions: vec![],
        moments: vec![],
        stems: None,
        waveform_path: None,
        partial: false,
    }
}
fn pair() -> (TrackAnalysis, TrackAnalysis) {
    (
        track(1, 128., "8A", S::Drop, 64, 0.9, 0.8),
        track(2, 126., "9A", S::Intro, 32, 0.8, 1.),
    )
}

fn plan(a: &TrackAnalysis, b: &TrackAnalysis) -> MixPlan {
    Planner::with_options(PlannerOptions {
        smooth: false,
        ..Default::default()
    })
    .plan_pair(
        a,
        b,
        &[],
        &[],
        PerformanceOffset::identity(),
        PerformanceOffset::identity(),
    )
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}
fn measured_phrases(t: &mut TrackAnalysis, bars: &[u32]) {
    t.phrase_boundaries = bars
        .iter()
        .map(|&bar| mixless_protocol::PhraseBoundary {
            time_sec: bar as f32 * 240. / t.tempo.global_bpm,
            confidence: 0.9,
            novelty: 0.5,
        })
        .collect();
}

#[test]
fn measured_phrases_select_an_eight_bar_overlap_when_a_long_blend_cuts_the_hook() {
    let mut a = track(1, 128., "8A", S::Outro, 64, 0.8, 0.2);
    let mut b = track(2, 128., "9A", S::Intro, 64, 0.8, 0.2);
    measured_phrases(&mut a, &[40, 48, 56, 64]);
    measured_phrases(&mut b, &[0, 8, 23, 39, 55]);
    // The outgoing phrase completes before the final eight-bar window.
    for bar in &mut a.bars[..56] {
        bar.vocal_presence = 0.8;
    }
    for bar in &mut b.bars[8..] {
        bar.vocal_presence = 0.8;
        bar.section = S::Verse;
    }
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(
        p.transition_mode,
        Some(mixless_protocol::TransitionMode::BeatBlend)
    );
    assert_eq!(p.summary.as_ref().unwrap().length_bars, 8);
    close(p.t_in_b, 0.);
    close(p.t_end_b, 15.);
}

#[test]
fn measured_phrase_boundaries_override_array_bar_numbers_and_preserve_future_candidates() {
    let mut a = track(1, 128., "8A", S::Outro, 128, 0.8, 0.2);
    let mut b = track(2, 128., "9A", S::Intro, 64, 0.8, 0.2);
    measured_phrases(&mut a, &(3..=123).step_by(8).collect::<Vec<_>>());
    measured_phrases(&mut b, &[3, 11, 19, 27, 35, 43, 51, 59]);
    for bar in &mut a.bars {
        bar.bar_index += 1;
    }
    let p = Planner::with_options(PlannerOptions {
        earliest_outgoing_sec: 180.,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.summary.is_some(), "{:?}", p.failure_reason);
    assert!(p.t_in_a >= 180.);
    for sec in [p.t_in_a, p.t_out_a] {
        assert!(a
            .phrase_boundaries
            .iter()
            .any(|b| (b.time_sec - sec).abs() < 0.01));
    }
    assert!(b
        .phrase_boundaries
        .iter()
        .any(|b| (b.time_sec - p.t_in_b).abs() < 0.01));
}

#[test]
fn a_completed_build_can_cut_to_a_drop_without_an_echo_tail() {
    let mut a = track(1, 128., "8A", S::BuildUp, 64, 0.8, 0.2);
    let mut b = track(2, 126., "3B", S::Drop, 64, 0.9, 0.2);
    // Explicitly measured ramp, not a flat phrase carrying a BuildUp label.
    for (i, bar) in a.bars.iter_mut().enumerate() {
        bar.rms = 0.05 + i as f32 * 0.004;
        bar.onset_density = 2. + i as f32;
        bar.high_db = -30. + i as f32 * 0.2;
    }
    measured_phrases(&mut a, &[48, 56, 64]);
    measured_phrases(&mut b, &[0, 8, 16, 24, 32, 40, 48, 56]);
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(p.summary.as_ref().unwrap().strategy, StrategyId::DropCut);
    assert!(!p.summary.as_ref().unwrap().used_fallback);
    close(p.lanes.fx_send_a.sample(3.9), 0.);
    close(p.lanes.filter_a.lp_hz.sample(3.9), 20000.);
    let n = p.summary.as_ref().unwrap().length_bars as f32;
    close(p.incoming_start_bar, n);
    let audible_cut = p.clock.sample(n)
        - p.clock.sample(
            p.lanes
                .xfader
                .nodes
                .iter()
                .find(|(_, v)| *v > -0.99)
                .unwrap()
                .0,
        );
    assert!(audible_cut < 0.1);
}

#[test]
fn malformed_boundary_confidence_cannot_authorize_a_transition() {
    let (mut a, b) = pair();
    measured_phrases(&mut a, &[32, 48, 64]);
    a.phrase_boundaries[1].confidence = f32::NAN;
    assert!(Planner::new()
        .plan_pair(&a, &b, &[], &[], Default::default(), Default::default())
        .summary
        .is_none());
}
