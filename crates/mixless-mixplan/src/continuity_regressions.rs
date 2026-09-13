use super::*;
use mixless_protocol::{MusicalMoment, SectionLabel as S};

fn moments(t: &mut TrackAnalysis, voice: f32) {
    t.moments = (0..(t.duration_sec * 20.).ceil() as usize)
        .map(|i| MusicalMoment {
            start_sec: i as f32 * 0.05,
            end_sec: ((i + 1) as f32 * 0.05).min(t.duration_sec),
            rms: 0.2,
            band_db: [-15., -18., -24.],
            chroma: std::array::from_fn(|i| if i == 0 { 1. } else { 0. }),
            onset: 0.2,
            attack_sec: i as f32 * 0.05,
            low_onset: 0.,
            sustain: 0.9,
            pitch_midi: Some(60.),
            vocal_confidence: Some(voice),
        })
        .collect();
    for bar in &mut t.bars {
        bar.vocal_confidence = Some(voice);
    }
}

#[test]
fn held_notes_are_not_cut_and_a_measured_breath_can_exit_within_a_vocal_section() {
    let mut a = track(1, 128., "8A", S::Verse, 64, 0.8, 0.2);
    moments(&mut a, 0.9);
    assert!(!crate::continuity::cut_safe(&a, 30., false));
    for m in a
        .moments
        .iter_mut()
        .filter(|m| m.start_sec >= 29.75 && m.end_sec <= 30.25)
    {
        m.rms = 0.003;
        m.band_db[1] = -50.;
    }
    assert!(crate::vocals::safe_exit(&a, 30.));
    // A pad can be continuous even if the human-voice classifier says no voice.
    for m in &mut a.moments {
        m.vocal_confidence = Some(0.);
    }
    assert!(!crate::continuity::cut_safe(&a, 45., false));
    assert!(crate::continuity::cut_safe(&a, 30., false));
    for m in a.moments.iter_mut().filter(|m| m.start_sec >= 45.) {
        m.pitch_midi = Some(64.);
        m.chroma = std::array::from_fn(|i| if i == 4 { 1. } else { 0. });
    }
    assert!(
        !crate::continuity::cut_safe(&a, 45., false),
        "a new note inside an ongoing melody is not an exit"
    );
}

#[test]
fn overlapping_vocals_can_exit_softly_without_waiting_for_the_entire_verse() {
    let mut a = track(1, 128., "8A", S::Outro, 64, 0.8, 0.2);
    let mut b = track(2, 128., "9A", S::Intro, 64, 0.8, 0.2);
    moments(&mut a, 0.9);
    moments(&mut b, 0.9);
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.failure_reason.is_none(), "{:?}", p.failure_reason);
    let n = p.summary.as_ref().unwrap().length_bars as f32;
    assert!(n >= 8.);
    assert_eq!(p.incoming_start_bar, 0.);
    let mut previous = 1.;
    for i in 0..=1000 {
        let u = n * i as f32 / 1000.;
        let angle = (p.lanes.xfader.sample(u) + 1.) * std::f32::consts::FRAC_PI_4;
        let va = angle.cos().max(0.)
            * 10f32.powf((p.lanes.gain_a.sample(u) + p.lanes.eq_a.mid.sample(u)) / 20.);
        let vb = angle.sin().max(0.)
            * 10f32.powf((p.lanes.gain_b.sample(u) + p.lanes.eq_b.mid.sample(u)) / 20.);
        assert!(va * vb < 0.085, "two foregrounds at {u}: {va} {vb}");
        assert!((va - previous).abs() < 0.035, "abrupt vocal exit at {u}");
        previous = va;
    }
    assert!(previous < 0.0001);
    assert!(p.t_out_a < a.duration_sec); // continuous voice still exists afterwards
}

#[test]
fn buildup_resolution_targets_the_incoming_drop_across_tempos_and_track_ids() {
    for bpm in [87., 128., 140., 174.] {
        let mut a = track(100 + bpm as i64, bpm, "8A", S::Outro, 96, 0.8, 0.2);
        let mut b = track(400 + bpm as i64, bpm, "3B", S::Intro, 96, 0.8, 0.2);
        let bar = 240. / bpm;
        a.sections = [
            (0., 32., S::Drop),
            (32., 40., S::Break),
            (40., 56., S::BuildUp),
            (56., 88., S::Drop),
            (88., 96., S::Outro),
        ]
        .into_iter()
        .map(|(x, y, label)| Section {
            start_sec: x * bar,
            end_sec: y * bar,
            label,
        })
        .collect();
        b.sections = [
            (0., 16., S::Intro),
            (16., 24., S::BuildUp),
            (24., 88., S::Drop),
            (88., 96., S::Outro),
        ]
        .into_iter()
        .map(|(x, y, label)| Section {
            start_sec: x * bar,
            end_sec: y * bar,
            label,
        })
        .collect();
        for (i, f) in a.bars.iter_mut().enumerate() {
            if (40..56).contains(&i) {
                f.onset_density = 2. + (i - 40) as f32;
                f.rms = 0.1 + (i - 40) as f32 * 0.02;
            }
        }
        assert!(crate::arrangement::breaks_build(
            &a,
            55. * bar,
            &b,
            24. * bar
        ));
        assert!(crate::arrangement::breaks_build(
            &a,
            56. * bar,
            &b,
            16. * bar
        ));
        let p = Planner::with_options(PlannerOptions {
            strategy: Some(StrategyId::DropCut),
            ..Default::default()
        })
        .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        assert!(p.failure_reason.is_none(), "{bpm}: {:?}", p.failure_reason);
        close(p.t_out_a, 56. * bar);
        close(p.t_in_b, 24. * bar);
        let n = p.summary.as_ref().unwrap().length_bars as f32;
        close(p.incoming_start_bar, n);
        close(n, 0.);
        close(p.t_in_a, p.t_out_a);
        close(p.lanes.xfader.sample(0.), 1.);
        close(p.lanes.fx_send_a.sample(n - 0.1), 0.);
        let late = Planner::with_options(PlannerOptions {
            strategy: Some(StrategyId::DropCut),
            earliest_outgoing_sec: 56. * bar - 0.04,
            ..Default::default()
        })
        .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        assert!(
            late.failure_reason.is_none(),
            "cut must not need one bar of pre-roll: {:?}",
            late.failure_reason
        );
        close(late.t_in_a, 56. * bar);
        assert_eq!(late.summary.unwrap().length_bars, 0);
    }
}

#[test]
fn low_band_attack_evidence_rejects_displaced_grids_and_missing_observations() {
    let mut a = track(1, 120., "8A", S::Intro, 16, 0.8, 0.2);
    moments(&mut a, 0.);
    for m in &mut a.moments {
        if (m.start_sec / 0.5 - (m.start_sec / 0.5).round()).abs() < 0.01 {
            m.low_onset = 1.;
        }
    }
    assert!(crate::rhythm::supports_grid(&a, 0., 8.));
    for m in &mut a.moments {
        m.attack_sec += 0.125;
    }
    assert!(!crate::rhythm::supports_grid(&a, 0., 8.));
    a.moments.clear();
    assert!(!crate::rhythm::supports_grid(&a, 0., 8.));
}

#[test]
fn filtered_background_cannot_reduce_the_foreground_before_its_drop_ends() {
    let mut a = track(1, 128., "8A", S::Outro, 64, 0.8, 0.2);
    let mut b = track(2, 128., "9A", S::Intro, 64, 0.2, 0.04);
    moments(&mut a, 0.);
    moments(&mut b, 0.);
    // Obtain a synchronized source clock, then place a measured peak across
    // its opening. The choreography must preserve it regardless of its length.
    b.bars.iter_mut().for_each(|b| b.kick_salience = 0.8);
    let mut p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let n = p.summary.as_ref().unwrap().length_bars as f32;
    let peak_end = p.outgoing_source.sample(n - 2.);
    a.sections = vec![
        Section {
            start_sec: 0.,
            end_sec: peak_end,
            label: S::Drop,
        },
        Section {
            start_sec: peak_end,
            end_sec: a.duration_sec,
            label: S::Break,
        },
    ];
    crate::filtered::arrange(&mut p, &a, &b).unwrap();
    for u in [1., n * 0.5, n - 2.01] {
        let x = (p.lanes.xfader.sample(u) + 1.) * std::f32::consts::FRAC_PI_4;
        close(x.cos() * 10f32.powf(p.lanes.gain_a.sample(u) / 20.), 1.);
        assert!(p.incoming_source.sample(u) > p.t_in_b);
        assert!(p.lanes.eq_b.low.sample(u) <= -90.);
        assert!(p.lanes.eq_b.mid.sample(u) <= -24.);
    }
    close(p.lanes.gain_a.sample(n), -96.);
    close(p.lanes.filter_b.hp_hz.sample(n), 20.);
}

#[test]
fn lookahead_never_retunes_the_audible_deck_or_overrides_a_failed_current_plan() {
    let (a, b) = pair();
    let c = track(3, 132., "3B", S::Intro, 64, 0.8, 0.2);
    let planner = Planner::with_options(PlannerOptions {
        harmonic_key_shift: true,
        ..Default::default()
    });
    let ctx = PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: PerformanceOffset {
            rate: 1.02,
            pitch_semitones: 1.,
        },
        offset_b: Default::default(),
    };
    let p = planner.plan_with_following(&ctx, Some(&c));
    assert!(p.failure_reason.is_none());
    assert!(p.lanes.pitch_a.nodes.iter().all(|(_, v)| *v == 1.));
    assert!(p
        .lanes
        .pitch_b
        .nodes
        .iter()
        .all(|(_, v)| *v == p.incoming_offset_end.pitch_semitones));
    let mut invalid = b.clone();
    invalid.duration_sec = f32::NAN;
    let invalid_ctx = PlanContext {
        incoming: &invalid,
        ..ctx
    };
    assert!(planner
        .plan_with_following(&invalid_ctx, Some(&c))
        .failure_reason
        .is_some());
}
