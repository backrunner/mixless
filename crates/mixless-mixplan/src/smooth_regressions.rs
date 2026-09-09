use super::{close, measured_phrases, track};
use crate::{musical, Planner, PlannerOptions};
use mixless_protocol::{Cue, CueKind, SectionLabel as S, StrategyId, TransitionMode};

fn cue(sec: f32) -> Cue {
    Cue {
        index: 0,
        frame: (sec * 48000.) as u64,
        kind: CueKind::In,
        user_set: true,
    }
}

#[test]
fn measured_bass_exchange_moves_off_midpoint_and_has_continuous_power() {
    let mut a = track(1, 120., "8A", S::Outro, 64, 0.9, 0.2);
    let mut b = track(2, 120., "9A", S::Intro, 64, 0.9, 0.2);
    measured_phrases(&mut a, &[48, 60, 64]);
    measured_phrases(&mut b, &[0, 12, 16]);
    let p = Planner::with_options(PlannerOptions {
        earliest_outgoing_sec: 96.,
        ..Default::default()
    })
    .plan_pair(
        &a,
        &b,
        &[cue(128.)],
        &[cue(0.)],
        Default::default(),
        Default::default(),
    );
    assert_eq!(p.transition_mode, Some(TransitionMode::BeatBlend));
    assert_eq!(p.summary.as_ref().unwrap().length_bars, 16);
    close(p.handoff_bar.unwrap(), 12.);
    close(p.lanes.xfader.sample(12.), 0.);
    for j in 0..=4096 {
        let u = 16. * j as f32 / 4096.;
        let x = (p.lanes.xfader.sample(u) + 1.) * std::f32::consts::FRAC_PI_4;
        let a = x.cos() * 10f32.powf(p.lanes.eq_a.low.sample(u) / 20.);
        let b = x.sin() * 10f32.powf(p.lanes.eq_b.low.sample(u) / 20.);
        assert!((0.96..=1.04).contains(&(a * a + b * b)), "power at {u}");
    }
    for lane in [&p.lanes.xfader, &p.lanes.eq_a.low, &p.lanes.eq_b.low] {
        assert!(lane.nodes.windows(2).all(|w| w[0].0 < w[1].0));
    }
}

#[test]
fn bass_swap_does_not_reveal_an_empty_low_end() {
    let a = track(1, 120., "8A", S::Outro, 64, 0.9, 0.2);
    let b = track(2, 120., "9A", S::Intro, 64, 0.1, 0.2);
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(p.transition_mode, Some(TransitionMode::PhraseBridge));
}

#[test]
fn late_drop_and_missing_peak_coverage_never_authorize_an_intro_boost() {
    let mut b = track(2, 120., "9A", S::Intro, 64, 0.8, 0.05);
    assert!(musical::incoming_trim(&b, 0., Some(0.2), Some(0.05)) > 5.9);
    b.bars[48].rms = 0.48;
    close(musical::incoming_trim(&b, 0., Some(0.2), Some(0.05)), 0.);
    b.bars[48].rms = 0.05;
    b.bars.remove(40);
    close(musical::incoming_trim(&b, 0., Some(0.2), Some(0.05)), 0.);
}

#[test]
fn local_level_is_weighted_by_overlap_duration_including_silence() {
    let mut t = track(1, 120., "8A", S::Outro, 2, 0.8, 0.2);
    t.bars[0].rms = 0.;
    t.bars[1].rms = 0.4;
    close(
        musical::average_rms(&t, 1., 4.).unwrap(),
        (0.16_f32 * 2. / 3.).sqrt(),
    );
}

#[test]
fn percussion_evidence_allows_layering_without_transposing_incompatible_keys() {
    let a = track(1, 128., "8A", S::Outro, 64, 0.8, 0.2);
    let mut b = track(2, 126., "3B", S::Intro, 64, 0.8, 0.2);
    for bar in &mut b.bars {
        bar.chroma = [1.; 12];
    }
    let plan =
        |b: &_| Planner::new().plan_pair(&a, b, &[], &[], Default::default(), Default::default());
    let p = plan(&b);
    assert_eq!(p.transition_mode, Some(TransitionMode::BeatBlend));
    assert_eq!(
        p.summary.as_ref().unwrap().strategy,
        StrategyId::PhraseBlend
    );
    close(p.lanes.pitch_b.sample(8.), 0.);
    for bar in &mut b.bars {
        bar.chroma = [0.; 12];
    }
    assert_eq!(plan(&b).transition_mode, Some(TransitionMode::PhraseBridge));
    for bar in &mut b.bars {
        bar.chroma = [1.; 12];
        bar.chroma[3] = 12.;
    }
    assert_eq!(plan(&b).transition_mode, Some(TransitionMode::PhraseBridge));
}

#[test]
fn a_long_mix_cannot_hide_one_colliding_vocal_line_in_its_average() {
    let mut a = track(1, 120., "8A", S::Outro, 64, 0.9, 0.2);
    let mut b = track(2, 120., "9A", S::Intro, 64, 0.9, 0.2);
    for bar in &mut a.bars {
        bar.vocal_presence = 0.9;
    }
    b.bars[10].vocal_presence = 0.9;
    let p = Planner::with_options(PlannerOptions {
        earliest_outgoing_sec: 64.,
        ..Default::default()
    })
    .plan_pair(
        &a,
        &b,
        &[cue(64.), cue(128.)],
        &[cue(0.), cue(64.)],
        Default::default(),
        Default::default(),
    );
    assert!(p.summary.is_none());
    assert!(p.failure_reason.is_some());
}

#[test]
fn malformed_features_cannot_bypass_musical_gates() {
    let a = track(1, 120., "8A", S::Outro, 64, 0.9, 0.2);
    let b = track(2, 120., "9A", S::Intro, 64, 0.9, 0.2);
    for damage in 0..3 {
        let mut broken = b.clone();
        match damage {
            0 => broken.bars[0].vocal_presence = f32::NAN,
            1 => broken.bars[0].chroma[2] = f32::NAN,
            _ => broken.bars[2].start_sec = 0.,
        }
        assert!(Planner::new()
            .plan_pair(
                &a,
                &broken,
                &[],
                &[],
                Default::default(),
                Default::default()
            )
            .summary
            .is_none());
    }
}

#[test]
fn sparse_phrases_use_a_gradual_low_band_blend_instead_of_a_drum_swap() {
    let a = track(1, 120., "8A", S::Break, 64, 0.1, 0.2);
    let b = track(2, 120., "9A", S::Intro, 64, 0.1, 0.2);
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(
        p.summary.as_ref().unwrap().strategy,
        StrategyId::PhraseBlend
    );
    let u = p.handoff_bar.unwrap();
    assert!(p.lanes.eq_a.low.sample(u + 0.5) > -12.);
    assert!(p.lanes.eq_b.low.sample(u - 0.5) > -12.);
    close(p.lanes.eq_a.high.sample(0.), 0.);
    close(p.lanes.eq_b.high.sample(0.), -3.);
    close(
        p.lanes
            .eq_b
            .high
            .sample(p.summary.as_ref().unwrap().length_bars as f32),
        0.,
    );
}

#[test]
fn explicit_loop_construct_uses_a_quantized_roll_and_release_plan() {
    let a = track(1, 128., "8A", S::Outro, 64, 0.8, 0.2);
    let b = track(2, 126., "8A", S::Intro, 64, 0.8, 0.2);
    let p = Planner::with_options(crate::PlannerOptions {
        strategy: Some(StrategyId::LoopConstruct),
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(p.transition_mode, Some(TransitionMode::LoopRoll));
    assert_eq!(
        p.summary.as_ref().unwrap().strategy,
        StrategyId::LoopConstruct
    );
    let op = p.lanes.loop_b.as_ref().expect("loop roll");
    assert!(op.length_bars == 1 || op.length_bars == 2);
    assert!(op.on_bar < op.off_bar && op.off_bar < p.summary.as_ref().unwrap().length_bars as f32);
    assert!(p.lanes.filter_a.lp_hz.sample(op.off_bar - 0.25) < 20000.);
    assert!(p.lanes.filter_a.lp_hz.sample(op.off_bar) <= 500.);
    assert_eq!(p.lanes.fx_send_a.sample(op.off_bar), 0.);
}

#[test]
fn explicit_scratch_cut_requires_a_hot_hit_and_compiles_three_phase_op() {
    let a = track(1, 128., "8A", S::Outro, 64, 0.9, 0.8);
    let b = track(2, 128., "9A", S::Intro, 32, 0.9, 0.8);
    let hot = Cue {
        index: 1,
        frame: 0,
        kind: CueKind::Hot,
        user_set: true,
    };
    let p = Planner::with_options(crate::PlannerOptions {
        strategy: Some(StrategyId::ScratchCut),
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[hot], Default::default(), Default::default());
    assert_eq!(p.summary.as_ref().unwrap().strategy, StrategyId::ScratchCut);
    let op = p.lanes.scratch_a.as_ref().expect("scratch op");
    assert!(op.on_bar < op.peak_bar && op.peak_bar < op.off_bar);
    assert!(op.peak_delta_frames < 0);
}

#[test]
fn catalog_scratch_cut_never_mislabels_a_drop_envelope() {
    let a = track(1, 128., "8A", S::Outro, 64, 0.9, 0.8);
    let b = track(2, 128., "9A", S::Intro, 32, 0.9, 0.8);
    let hot = Cue {
        index: 1,
        frame: 0,
        kind: CueKind::Hot,
        user_set: true,
    };
    let p = Planner::with_options(crate::PlannerOptions {
        smooth: false,
        strategy: Some(StrategyId::ScratchCut),
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[hot], Default::default(), Default::default());
    assert_eq!(p.summary.as_ref().unwrap().strategy, StrategyId::ScratchCut);
    assert!(p.lanes.scratch_a.is_some());
}
