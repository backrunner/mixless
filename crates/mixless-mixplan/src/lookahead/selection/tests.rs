use super::*;
use mixless_protocol::{MixPlanSummary, Polyline, SectionLabel, StrategyId};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn same_endpoints_keep_better_routing_and_share_only_the_departure() {
    let a = crate::tests::track(1, 120., "8A", SectionLabel::Outro, 64, 0.8, 0.2);
    let b = crate::tests::track(2, 120., "8A", SectionLabel::Intro, 72, 0.8, 0.2);
    let ctx = PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    let original = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: StrategyId::PhraseBlend,
            score: 0.6,
            used_fallback: false,
            length_bars: 8,
        }),
        t_out_a: 16.,
        t_end_b: 16.,
        outgoing_source: Polyline {
            nodes: vec![(0., 0.), (8., 16.)],
        },
        incoming_source: Polyline {
            nodes: vec![(0., 0.), (8., 16.)],
        },
        ..Default::default()
    };
    let mut better = original.clone();
    better.summary.as_mut().unwrap().score = 0.8;
    better.lanes.eq_a.low = Polyline::constant(-12.);
    // The old endpoint comparison would discard `better` before ranking.
    assert_eq!(original.t_in_a, better.t_in_a);
    assert_eq!(original.t_out_a, better.t_out_a);
    assert_eq!(original.t_in_b, better.t_in_b);
    assert_eq!(original.t_end_b, better.t_end_b);
    assert_eq!(original.incoming_offset_end, better.incoming_offset_end);
    let calls = AtomicUsize::new(0);
    let selected = choose(
        &ctx,
        vec![original.clone()],
        [better.clone(), original.clone()],
        |_| {},
        |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            0.2
        },
    );
    assert_eq!(selected.summary.unwrap().score, 0.8);
    assert_eq!(selected.lanes.eq_a.low.sample(0.), -12.);
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    // Equal musical cost retains the earlier route, and a failed route cannot
    // displace it even if its stale summary advertises a higher score.
    better.summary.as_mut().unwrap().score = 0.6;
    let mut failed = better.clone();
    failed.summary.as_mut().unwrap().score = 1.;
    failed.failure_reason = Some("unavailable source evidence".into());
    let selected = choose(
        &ctx,
        vec![original.clone()],
        [better, failed],
        |_| {},
        |_| panic!("dominated identical states need no departure forecast"),
    );
    assert_eq!(
        selected.lanes.eq_a.low.sample(0.),
        original.lanes.eq_a.low.sample(0.)
    );
}
