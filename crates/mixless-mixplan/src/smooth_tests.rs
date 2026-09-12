use super::*;

#[test]
fn smooth_policy_transfers_tempo_and_keeps_low_end_coherent() {
    let (a, b) = pair();
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let summary = p.summary.as_ref().expect("smooth plan");
    assert_eq!(summary.strategy, StrategyId::BassSwap);
    assert_eq!(
        p.transition_mode,
        Some(mixless_protocol::TransitionMode::BeatBlend)
    );
    assert!(p.lanes.rate_a.sample(0.) > p.lanes.rate_a.sample(summary.length_bars as f32));
    assert!((p.lanes.rate_b.sample(0.) - 128. / 126.).abs() < 0.002);
    assert!((p.lanes.rate_b.sample(summary.length_bars as f32) - 1.).abs() < 0.002);
    let middle = p.handoff_bar.expect("phrase-aligned bass exchange");
    assert!(p.lanes.eq_a.low.sample(middle - 0.25) > -12.);
    assert!(p.lanes.eq_a.low.sample(middle + 0.25) < -80.);
    assert!(p.lanes.eq_b.low.sample(middle - 0.25) < -80.);
    assert!(p.lanes.eq_b.low.sample(middle + 0.25) > -12.);
}

#[test]
fn smooth_policy_uses_short_phrase_bridge_for_unreliable_grid_or_harmonic_conflict() {
    let (mut a, mut b) = pair();
    a.tempo.beats.clear();
    a.tempo.downbeats.clear();
    a.key_confidence = 0.1;
    b.key_confidence = 0.1;
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let summary = p.summary.as_ref().expect("bridge plan");
    assert_eq!(
        p.transition_mode,
        Some(mixless_protocol::TransitionMode::PhraseBridge)
    );
    assert!(summary.used_fallback);
    assert!(summary.length_bars <= 4);
    assert_eq!(p.bar_map, BarMap::OneToOne);
    assert!((p.lanes.rate_b.sample(summary.length_bars as f32) - 1.).abs() < 0.002);
}

#[test]
fn smooth_policy_allows_pairwise_bpm_transfer_without_playlist_lock() {
    let (a, b) = pair();
    let mut c = track(3, 132., "10A", S::Intro, 64, 0.8, 1.);
    let first = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let second = Planner::new().plan_pair(
        &b,
        &c,
        &[],
        &[],
        first.incoming_offset_end,
        Default::default(),
    );
    assert!(first.summary.is_some() && second.summary.is_some());
    assert!((second.lanes.rate_b.sample(0.) - 126. / 132.).abs() < 0.04);
    assert!((second.incoming_offset_end.rate - 1.).abs() < 0.01);
    c = track(3, 80., "9A", S::Intro, 64, 0.8, 1.);
    let fast = track(4, 160., "8A", S::Drop, 64, 0.9, 0.8);
    let half =
        Planner::new().plan_pair(&fast, &c, &[], &[], Default::default(), Default::default());
    assert_eq!(
        half.bar_map,
        BarMap::TwoToOne {
            outgoing_is_double: true
        }
    );
    assert!(half.lanes.rate_b.sample(0.) < 1.2);
}

#[test]
fn smooth_tempo_slope_phase_and_bass_power_are_bounded_in_both_directions() {
    for (bpm_a, bpm_b) in [(128., 136.), (136., 128.), (160., 80.), (80., 160.)] {
        let a = track(1, bpm_a, "8A", S::Outro, 64, 0.9, 0.2);
        let b = track(2, bpm_b, "9A", S::Intro, 64, 0.8, 0.2);
        let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        assert_eq!(
            p.transition_mode,
            Some(mixless_protocol::TransitionMode::BeatBlend)
        );
        let n = p.summary.as_ref().unwrap().length_bars as f32;
        let factor = match p.bar_map {
            BarMap::TwoToOne {
                outgoing_is_double: true,
            } => 0.5,
            BarMap::TwoToOne {
                outgoing_is_double: false,
            } => 2.,
            _ => 1.,
        };
        for points in p.clock.nodes.windows(2) {
            let [(u0, t0), (u1, t1)] = [points[0], points[1]];
            let slope = (p.master_bpm.sample(u1) / p.master_bpm.sample(u0))
                .ln()
                .abs()
                / (t1 - t0);
            assert!(slope <= 0.0036, "tempo slope {slope}");
        }
        for j in 0..=2048 {
            let u = n * j as f32 / 2048.;
            let beat_a = (p.outgoing_source.sample(u) - p.t_in_a) * bpm_a / 60.;
            let beat_b = (p.incoming_source.sample(u) - p.t_in_b) * bpm_b / 60.;
            assert!((beat_b - beat_a * factor).abs() < 0.001);
            let angle = (p.lanes.xfader.sample(u) + 1.) * std::f32::consts::FRAC_PI_4;
            let low_a = angle.cos() * 10f32.powf(p.lanes.eq_a.low.sample(u) / 20.);
            let low_b = angle.sin() * 10f32.powf(p.lanes.eq_b.low.sample(u) / 20.);
            let power = low_a * low_a + low_b * low_b;
            assert!((0.96..=1.04).contains(&power), "bass power {power} at {u}");
        }
        close(p.incoming_offset_end.rate, 1.);
        close(p.lanes.pitch_a.sample(n), 0.);
        close(p.lanes.pitch_b.sample(n), 0.);
    }
}

#[test]
fn incompatible_bridge_overlaps_on_phrase_boundary_at_native_tempo_and_key() {
    let a = track(1, 128., "8A", S::Outro, 64, 0.9, 0.2);
    let b = track(2, 105., "3B", S::Intro, 64, 0.8, 0.2);
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(
        p.transition_mode,
        Some(mixless_protocol::TransitionMode::PhraseBridge)
    );
    let n = p.summary.as_ref().unwrap().length_bars as f32;
    close(p.incoming_start_bar, 0.);
    close(p.clock.sample(n), p.t_out_a - p.t_in_a);
    close(p.duration_sec(), p.clock.sample(n));
    close(p.t_end_b - p.t_in_b, p.duration_sec());
    assert!(p.duration_sec() >= 3.);
    assert!(p.lanes.xfader.sample(n * 0.25) > -0.95);
    assert!(p.lanes.xfader.sample(n * 0.75) < 0.95);
    close(p.lanes.xfader.sample(n), 1.);
    close(p.lanes.gain_a.sample(n), -96.);
    close(p.lanes.rate_a.sample(n), 1.);
    close(p.lanes.rate_b.sample(n), 1.);
    close(p.lanes.pitch_b.sample(n), 0.);
}

#[test]
fn smooth_safety_gates_include_vocal_exclusions_cues_and_local_grid_failures() {
    let mut a = track(1, 128., "8A", S::Verse, 64, 0.9, 0.2);
    let mut b = track(2, 126., "9A", S::Chorus, 64, 0.8, 0.2);
    for bar in a.bars.iter_mut().chain(&mut b.bars) {
        bar.vocal_presence = 0.9;
    }
    let planner = Planner::new();
    assert!(planner
        .plan_pair(&a, &b, &[], &[], Default::default(), Default::default())
        .summary
        .is_none());
    let (mut a, b) = pair();
    for segment in &mut a.tempo.segments {
        segment.confidence = 0.4;
    }
    let p = planner.plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(
        p.transition_mode,
        Some(mixless_protocol::TransitionMode::PhraseBridge)
    );
    let impossible = Cue {
        index: 0,
        frame: (b.duration_sec * b.sample_rate as f32) as u64 + 48000,
        kind: CueKind::In,
        user_set: true,
    };
    assert!(planner
        .plan_pair(
            &a,
            &b,
            &[],
            &[impossible],
            Default::default(),
            Default::default()
        )
        .summary
        .is_none());
}

#[test]
fn smooth_energy_hold_then_release_carries_sounding_offsets() {
    let (a, b) = pair();
    let offset = PerformanceOffset {
        rate: 1.02,
        pitch_semitones: 1.,
    };
    let incoming = PerformanceOffset {
        rate: 1.,
        pitch_semitones: 1.,
    };
    let held = Planner::with_options(PlannerOptions {
        strategy: Some(StrategyId::EnergyHold),
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], offset, incoming);
    assert_eq!(
        held.summary.as_ref().unwrap().strategy,
        StrategyId::EnergyHold
    );
    close(held.lanes.rate_a.sample(0.), offset.rate);
    close(held.incoming_offset_end.rate, 128. * offset.rate / 126.);
    close(held.incoming_offset_end.pitch_semitones, 1.);
    let mut c = track(3, 132., "9A", S::Intro, 64, 0.8, 0.2);
    c.key_confidence = 1.;
    let next = Planner::new().plan_pair(&b, &c, &[], &[], held.incoming_offset_end, incoming);
    assert_eq!(
        next.transition_mode,
        Some(mixless_protocol::TransitionMode::BeatBlend)
    );
    close(next.lanes.rate_a.sample(0.), held.incoming_offset_end.rate);
    close(
        next.lanes.pitch_a.sample(0.),
        held.incoming_offset_end.pitch_semitones,
    );
    close(next.incoming_offset_end.rate, 1.);
}

#[test]
fn smooth_skips_measured_leading_silence_and_rejects_silent_tracks() {
    let (a, mut b) = pair();
    for bar in &mut b.bars[..8] {
        bar.rms = 0.;
        bar.vocal_presence = 0.;
    }
    let planner = Planner::new();
    let p = planner.plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.summary.is_some());
    assert!(p.t_in_b >= b.bars[8].start_sec - 0.001);
    for bar in &mut b.bars {
        bar.rms = 0.;
    }
    assert!(planner
        .plan_pair(&a, &b, &[], &[], Default::default(), Default::default())
        .summary
        .is_none());
}
