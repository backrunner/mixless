use super::{close, measured_phrases, track};
use crate::{Planner, PlannerOptions};
use mixless_protocol::{Section, SectionLabel as S, StrategyId};

#[test]
fn first_drop_exit_is_preferred_but_a_late_pass_still_has_a_transition() {
    let mut a = track(1, 120., "8A", S::Outro, 96, 0.8, 0.2);
    let b = track(2, 120., "9A", S::Intro, 96, 0.8, 0.2);
    a.sections = [
        (0., 32., S::Intro),
        (32., 64., S::Drop),
        (64., 96., S::Break),
        (96., 128., S::Drop),
        (128., 192., S::Outro),
    ]
    .into_iter()
    .map(|(start_sec, end_sec, label)| Section {
        start_sec,
        end_sec,
        label,
    })
    .collect();
    for bar in &mut a.bars {
        bar.section = a
            .sections
            .iter()
            .find(|s| bar.start_sec >= s.start_sec && bar.start_sec < s.end_sec)
            .unwrap()
            .label;
    }
    measured_phrases(&mut a, &[0, 8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96]);
    let plan = |earliest, entry| {
        Planner::with_options(PlannerOptions {
            earliest_outgoing_sec: earliest,
            outgoing_entry_sec: entry,
            ..Default::default()
        })
        .plan_pair(&a, &b, &[], &[], Default::default(), Default::default())
    };
    let p = plan(0., 0.);
    assert!(p.summary.is_some(), "{:?}", p.failure_reason);
    assert!((64. ..=96.).contains(&p.t_out_a), "exit {}", p.t_out_a);
    let late = plan(110., 0.);
    assert!(late.summary.is_some() && late.t_in_a >= 110.);
    assert_eq!(crate::drops::penalty(&a, 128., 96.), 0.);
    assert!(crate::drops::penalty(&a, 128., 0.) > 0.3);
}

#[test]
fn incompatible_drums_use_a_true_cut_instead_of_a_one_bar_filter_or_echo_blend() {
    for label in [S::Drop, S::Outro] {
        let a = track(1, 128., "8A", label, 64, 0.8, 0.2);
        let b = track(2, 105., "3B", S::Intro, 64, 0.8, 0.2);
        let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        let summary = p.summary.as_ref().unwrap();
        assert_eq!(summary.strategy, StrategyId::DryCut);
        let n = summary.length_bars as f32;
        close(p.incoming_start_bar, n);
        close(n, 0.);
        close(p.t_in_a, p.t_out_a);
        close(p.lanes.xfader.sample(n), 1.);
        close(p.lanes.fx_send_a.sample(n), 0.);
        close(p.lanes.filter_a.lp_hz.sample(n), 20000.);
        close(p.lanes.rate_b.sample(n), 1.);
        assert!(p.duration_sec() < 1.); // transport settling, no musical pre-roll
    }
}

fn structured(
    break_bars: u32,
) -> (
    mixless_protocol::TrackAnalysis,
    mixless_protocol::TrackAnalysis,
) {
    let mut a = track(1, 174., "8A", S::Outro, 128, 0.8, 0.2);
    let mut b = track(2, 174., "9A", S::Intro, 128, 0.8, 0.2);
    let sec = 240. / 174.;
    a.sections = [
        (0, 16, S::Intro),
        (16, 32, S::Drop),
        (32, 48, S::Drop),
        (48, 48 + break_bars, S::Break),
        (48 + break_bars, 112, S::Drop),
        (112, 128, S::Outro),
    ]
    .into_iter()
    .map(|(a, b, label)| Section {
        start_sec: a as f32 * sec,
        end_sec: b as f32 * sec,
        label,
    })
    .collect();
    for bar in &mut a.bars {
        bar.section = a
            .sections
            .iter()
            .find(|s| bar.start_sec >= s.start_sec - 0.001 && bar.start_sec < s.end_sec - 0.001)
            .unwrap()
            .label;
    }
    measured_phrases(
        &mut a,
        &[
            0,
            8,
            16,
            24,
            32,
            40,
            48,
            48 + break_bars / 2,
            48 + break_bars,
            112,
            120,
            128,
        ],
    );
    measured_phrases(&mut b, &[0, break_bars / 2, break_bars, 112, 128]);
    (a, b)
}

#[test]
fn recovery_overlap_preserves_both_halves_of_drop_and_adapts_to_available_phrases() {
    let mut lengths = std::collections::BTreeSet::new();
    for bars in [8, 16, 24] {
        let (a, b) = structured(bars);
        let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        let summary = p.summary.as_ref().expect("recovery blend");
        assert!(
            p.t_in_a + 0.01 >= a.sections[2].end_sec,
            "{summary:?}, start {}",
            p.t_in_a
        );
        assert!(p.t_out_a <= a.sections[3].end_sec + 0.01);
        // Finishing on a matching incoming phrase may use part of the break.
        // Require a real overlap, bounded by the available recovery interval.
        assert!(
            (4..=bars as u16).contains(&summary.length_bars),
            "{summary:?}"
        );
        lengths.insert(summary.length_bars);
        assert_eq!(p.incoming_start_bar, 0.);
        let n = summary.length_bars as f32;
        assert!(p.lanes.xfader.sample(n * 0.5).abs() < 0.9);
        assert!(p.lanes.gain_a.sample(n * 0.5) > -12. && p.lanes.gain_b.sample(n * 0.5) > -12.);
    }
    assert!(lengths.len() >= 2, "every recovery used {lengths:?}");
}

#[test]
fn later_or_partially_entered_drop_cannot_be_faded_halfway_through() {
    let (a, _) = structured(16);
    assert!(!crate::drops::allows(&a, 24., 42., 0., false));
    assert!(crate::drops::allows(
        &a,
        48. * 240. / 174. - 8.,
        48. * 240. / 174.,
        0.,
        false
    ));
    assert!(!crate::drops::allows(&a, 90., 110., 0., false));
    assert!(!crate::drops::allows(&a, 80., 90., 30., true));
}

#[test]
fn terminal_drop_fallback_holds_to_the_end_before_launching_incoming() {
    let (mut a, b) = structured(16);
    a.sections = vec![
        a.sections[0].clone(),
        Section {
            start_sec: 16. * 240. / 174.,
            end_sec: a.duration_sec,
            label: S::Drop,
        },
    ];
    let ctx = crate::PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    let p = crate::short_handoff(&ctx, a.duration_sec - 3.);
    assert!(p.failure_reason.is_none());
    assert_eq!(p.summary.as_ref().unwrap().strategy, StrategyId::DryCut);
    close(p.t_out_a, a.duration_sec);
    close(p.incoming_start_bar, 1.);
    close(p.lanes.xfader.sample(0.95), -1.);
}
