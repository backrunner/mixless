use super::{measured_phrases, track};
use crate::{Planner, PlannerOptions};
use mixless_protocol::{MixPlan, SectionLabel as S, TransitionMode};
fn plan(bars: u32) -> MixPlan {
    let mut a = track(1, 120., "8A", S::Outro, 128, 0.9, 0.2);
    let mut b = track(2, 120., "8A", S::Intro, 128, 0.9, 0.2);
    measured_phrases(&mut a, &[128 - bars, 128 - bars / 2, 128]);
    measured_phrases(&mut b, &[0, bars / 2, bars]);
    Planner::with_options(PlannerOptions {
        earliest_outgoing_sec: (128 - bars) as f32 * 2.,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default())
}
#[test]
fn measured_spans_select_24_48_and_64_bar_blends_without_a_fixed_duration() {
    for bars in [24, 48, 64] {
        let p = plan(bars);
        assert_eq!(p.transition_mode, Some(TransitionMode::BeatBlend));
        assert_eq!(
            p.summary.as_ref().unwrap().length_bars,
            bars as u16,
            "{p:?}"
        );
        assert!((p.duration_sec() - bars as f32 * 2.).abs() < 0.01);
        assert!(p.t_end_b < 256. - 16.);
        assert!(p.stages.len() >= 5);
        for stage in &p.stages {
            assert!(stage.start_bar < stage.end_bar);
            let at = (p.clock.sample(stage.start_bar) + p.clock.sample(stage.end_bar)) * 0.5;
            assert_eq!(p.stage_at_elapsed(at).unwrap().label, stage.label);
        }
    }
}
#[test]
fn long_blend_holds_the_mixer_and_exchanges_high_bass_mid_in_separate_phases() {
    let p = plan(48);
    let lanes = &p.lanes;
    assert_eq!(lanes.xfader.sample(12.), 0.);
    assert_eq!(lanes.xfader.sample(32.), 0.);
    assert!(lanes.eq_b.high.sample(23.) > 0. && lanes.eq_b.mid.sample(23.) < -80.);
    assert!(lanes.eq_b.low.sample(25.) > 0. && lanes.eq_a.mid.sample(25.) > 0.);
    assert!(lanes.eq_a.high.sample(23.) < -80. && lanes.eq_a.low.sample(25.) < -80.);
    assert!(lanes.gain_b.sample(0.) <= -90. && lanes.gain_b.sample(4.) > -0.01);
    assert!(lanes.gain_a.sample(48.) <= -90.);
    assert_eq!(lanes.filter_a.lp_hz.sample(47.), 20000.);
    // A clean instrumental blend has no gratuitous echo.
    assert!(lanes.fx_send_a.nodes.iter().all(|(_, v)| *v == 0.));
    // Equal-power targets include physical levels and crossfader attenuation.
    for u in (0..48 * 256).map(|j| j as f32 / 256.) {
        let x = (lanes.xfader.sample(u) + 1.) * std::f32::consts::FRAC_PI_4;
        let a = x.cos() * 10f32.powf(lanes.gain_a.sample(u) / 20.);
        let b = x.sin() * 10f32.powf(lanes.gain_b.sample(u) / 20.);
        for (ea, eb) in [
            (&lanes.eq_a.low, &lanes.eq_b.low),
            (&lanes.eq_a.mid, &lanes.eq_b.mid),
            (&lanes.eq_a.high, &lanes.eq_b.high),
        ] {
            let power = (a * 10f32.powf(ea.sample(u) / 20.)).powi(2)
                + (b * 10f32.powf(eb.sample(u) / 20.)).powi(2);
            assert!((0.94..1.04).contains(&power), "bar {u}, power {power}");
        }
    }
    // Opening curve eases in, instead of moving at constant speed.
    let early = lanes.xfader.sample(0.5) - lanes.xfader.sample(0.);
    let middle = lanes.xfader.sample(2.) - lanes.xfader.sample(1.5);
    assert!(middle > early * 3.);
}
#[test]
fn clean_long_blend_does_not_insert_effects_just_because_the_track_is_an_outro() {
    let mut a = track(1, 120., "8A", S::Outro, 128, 0.9, 0.2);
    let b = track(2, 120., "8A", S::Intro, 128, 0.9, 0.2);
    for bar in &mut a.bars {
        bar.vocal_presence = 0.2;
    }
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.summary.as_ref().unwrap().length_bars >= 24);
    assert!(p.lanes.fx_send_a.nodes.iter().all(|(_, v)| *v == 0.));
    let n = p.summary.as_ref().unwrap().length_bars as f32;
    assert_eq!(p.lanes.fx_send_a.sample(n), 0.);
    assert!(p
        .lanes
        .fx_send_a
        .nodes
        .iter()
        .filter(|(u, _)| *u < n - 0.5)
        .all(|(_, v)| *v == 0.));
}

#[test]
fn explicit_entry_before_the_first_measured_beat_uses_the_actual_cue() {
    let a = track(1, 120., "8A", S::Outro, 64, 0.9, 0.2);
    let mut b = track(2, 120., "8A", S::Intro, 64, 0.9, 0.2);
    for t in b.tempo.beats.iter_mut().chain(&mut b.tempo.downbeats) {
        *t += 0.4;
    }
    b.duration_sec += 0.4;
    for bar in &mut b.bars {
        bar.start_sec += 0.4;
        bar.end_sec += 0.4;
    }
    b.sections[0].end_sec += 0.4;
    let cue = mixless_protocol::Cue {
        index: 0,
        frame: 0,
        kind: mixless_protocol::CueKind::In,
        user_set: true,
    };
    let p = Planner::new().plan_pair(&a, &b, &[], &[cue], Default::default(), Default::default());
    assert_eq!(p.transition_mode, Some(TransitionMode::PhraseBridge));
    assert_eq!(p.t_in_b, 0.);
    assert_eq!(p.lanes.rate_b.sample(0.), 1.);
    assert_eq!(p.lanes.pitch_b.sample(0.), 0.);
}
