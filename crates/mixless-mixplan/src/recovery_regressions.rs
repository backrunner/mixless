use super::*;

fn vocal_break() -> (TrackAnalysis, TrackAnalysis) {
    let mut a = track(1, 174., "8A", S::Drop, 128, 0.9, 0.2);
    let mut b = track(2, 174., "8A", S::Intro, 128, 0.9, 0.2);
    let bar = 240. / 174.;
    a.sections = [
        (0., 32., S::Intro),
        (32., 64., S::Drop),
        (64., 72., S::Breakdown),
        (72., 80., S::BuildUp),
        (80., 112., S::Drop),
        (112., 128., S::Outro),
    ]
    .into_iter()
    .map(|(start, end, label)| Section {
        start_sec: start * bar,
        end_sec: end * bar,
        label,
    })
    .collect();
    measured_phrases(&mut a, &[0, 32, 64, 72, 80, 112, 128]);
    measured_phrases(&mut b, &[0, 16, 32, 64, 96, 128]);
    for (i, f) in a.bars.iter_mut().enumerate() {
        f.section = a
            .sections
            .iter()
            .find(|s| f.start_sec >= s.start_sec && f.start_sec < s.end_sec)
            .unwrap()
            .label;
        f.vocal_confidence = Some(if (66..74).contains(&i) { 0.8 } else { 0. });
        f.vocal_presence = 0.85; // synth foreground is separate from voice
        if (64..76).contains(&i) {
            f.kick_salience = 0.1;
        }
    }
    for f in &mut b.bars {
        f.vocal_confidence = Some(0.);
        f.vocal_presence = 0.8;
    }
    a.tempo.segments = [(0., 256., 1.), (256., 304., 0.1), (304., 512., 1.)]
        .into_iter()
        .map(|(start_beat, end_beat, confidence)| TempoSegment {
            start_beat,
            end_beat,
            bpm: 174.,
            confidence,
        })
        .collect();
    (a, b)
}

#[test]
fn explicit_recovery_window_layers_next_track_and_preserves_the_sung_line() {
    let (a, b) = vocal_break();
    let bar = 240. / 174.;
    let cue = mixless_protocol::Cue {
        index: 1,
        kind: mixless_protocol::CueKind::Out,
        frame: (75. * bar * a.sample_rate as f32) as u64,
        user_set: true,
    };
    let p = Planner::with_options(PlannerOptions {
        earliest_outgoing_sec: 64. * bar,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[cue], &[], Default::default(), Default::default());
    assert!(p.failure_reason.is_none(), "{:?}", p.failure_reason);
    assert!(
        (64. * bar - 0.05..=67. * bar).contains(&p.t_in_a),
        "{}..{} {:?} windows={:?} voices={:?}",
        p.t_in_a,
        p.t_out_a,
        p.summary,
        crate::recovery::windows(&a),
        crate::vocals::phrases(&a)
    );
    assert!(p.t_out_a <= 75. * bar + 0.05, "{}", p.t_out_a);
    assert!(
        p.summary.as_ref().unwrap().length_bars >= 8,
        "{:?}",
        p.summary
    );
    assert_eq!(p.incoming_start_bar, 0.);
    let u = (73. * bar - p.t_in_a) / bar;
    let angle = (p.lanes.xfader.sample(u) + 1.) * std::f32::consts::FRAC_PI_4;
    let voice_level = angle.cos() * 10f32.powf(p.lanes.gain_a.sample(u) / 20.);
    assert!((voice_level - 1.).abs() < 0.005, "{voice_level}");
    assert!(angle.sin() * 10f32.powf(p.lanes.gain_b.sample(u) / 20.) > 0.15);
    assert_eq!(p.lanes.eq_a.mid.sample(u), 0.);
    assert_eq!(p.lanes.filter_a.lp_hz.sample(u), 20000.);
    assert_eq!(p.lanes.fx_send_a.sample(u), 0.);
    assert!(p.incoming_source.sample(u) > p.t_in_b + bar);
    for i in 0..100 {
        let u = p.summary.as_ref().unwrap().length_bars as f32 * i as f32 / 100.;
        let x = (p.lanes.xfader.sample(u) + 1.) * std::f32::consts::FRAC_PI_4;
        let low_a =
            x.cos() * 10f32.powf((p.lanes.gain_a.sample(u) + p.lanes.eq_a.low.sample(u)) / 20.);
        let low_b =
            x.sin() * 10f32.powf((p.lanes.gain_b.sample(u) + p.lanes.eq_b.low.sample(u)) / 20.);
        assert!(low_a * low_a + low_b * low_b <= 1.02);
    }
}

#[test]
fn sparse_clock_needs_agreeing_anchors_and_cannot_hide_off_grid_drums() {
    let (mut a, _) = vocal_break();
    let bar = 240. / 174.;
    assert!(crate::smooth::grid_reliable(&a, 65. * bar, 75. * bar));
    a.tempo.segments[2].bpm = 176.;
    assert!(!crate::smooth::grid_reliable(&a, 65. * bar, 75. * bar));
    a.tempo.segments[2].bpm = 174.;
    a.bars[70].kick_salience = 0.9;
    assert!(!crate::smooth::grid_reliable(&a, 65. * bar, 75. * bar));
    a.bars[70].kick_salience = 0.1;
    a.tempo.segments.pop();
    assert!(!crate::smooth::grid_reliable(&a, 65. * bar, 75. * bar));
}

#[test]
fn a_two_bar_breath_with_voice_support_does_not_create_a_false_exit() {
    let (mut a, _) = vocal_break();
    a.bars[69].vocal_confidence = Some(0.3);
    a.bars[70].vocal_confidence = Some(0.28);
    assert!(!crate::vocals::safe_exit(&a, a.bars[70].start_sec));
    assert!(!crate::vocals::safe_entry(&a, a.bars[71].start_sec));
    assert!(crate::vocals::safe_exit(&a, a.bars[74].start_sec));
}

#[test]
fn filtered_rhythm_requires_voice_evidence_and_hides_tonal_bands_until_the_handoff() {
    let (mut a, mut b) = vocal_break();
    // Give the breakdown room for a complete filtered phrase: a blend must
    // resolve well before the next drop instead of consuming its downbeat.
    let bar = 240. / 174.;
    a.sections = [
        (0., 32., S::Intro),
        (32., 64., S::Drop),
        (64., 88., S::Breakdown),
        (88., 96., S::BuildUp),
        (96., 120., S::Drop),
        (120., 128., S::Outro),
    ]
    .into_iter()
    .map(|(start, end, label)| Section {
        start_sec: start * bar,
        end_sec: end * bar,
        label,
    })
    .collect();
    for f in &mut a.bars {
        f.section = a
            .sections
            .iter()
            .find(|s| f.start_sec >= s.start_sec && f.start_sec < s.end_sec)
            .unwrap()
            .label;
    }
    b.camelot = Some("11A".into());
    assert!(crate::filtered::eligible(&b, 0., 16.));
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.failure_reason.is_none(), "{:?}", p.failure_reason);
    assert_eq!(
        p.summary.as_ref().unwrap().strategy,
        StrategyId::FilterSweep
    );
    let n = p.summary.as_ref().unwrap().length_bars as f32;
    assert!(n >= 4.);
    assert_eq!(p.incoming_start_bar, 0.);
    assert!(p.lanes.eq_b.mid.sample(n * 0.5) <= -24.);
    assert!(p.lanes.eq_b.low.sample(n * 0.5) <= -90.);
    assert!((900. ..=2500.).contains(&p.lanes.filter_b.hp_hz.sample(n * 0.5)));
    assert!((p.lanes.filter_b.hp_hz.sample(n) - 20.).abs() < 0.01);
    assert_eq!(p.lanes.eq_b.mid.sample(n), 0.);
    assert!(p
        .lanes
        .xfader
        .nodes
        .iter()
        .all(|(_, v)| (-1. ..=1.).contains(v)));
    assert!(p
        .lanes
        .filter_b
        .hp_hz
        .nodes
        .iter()
        .all(|(_, v)| (20. ..=20000.).contains(v)));
    for bar in &mut b.bars {
        bar.vocal_confidence = None;
    }
    assert!(!crate::filtered::eligible(&b, 0., 16.));
    for bar in &mut b.bars {
        bar.vocal_confidence = Some(0.8);
    }
    assert!(!crate::filtered::eligible(&b, 0., 16.));
}
