use super::*;

#[test]
fn variants_use_the_same_audible_merit_as_the_outer_search() {
    let a = crate::tests::track(
        1,
        120.,
        "8A",
        mixless_protocol::SectionLabel::Outro,
        64,
        0.9,
        0.2,
    );
    let b = crate::tests::track(
        2,
        120.,
        "8A",
        mixless_protocol::SectionLabel::Intro,
        64,
        0.9,
        0.2,
    );
    let ctx = PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    let mut cut = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: S::PhraseBlend,
            score: 0.6,
            used_fallback: false,
            length_bars: 8,
        }),
        t_in_a: 112.,
        t_out_a: 128.,
        t_in_b: 0.,
        t_end_b: 16.,
        clock: Polyline {
            nodes: vec![(0., 0.), (8., 16.)],
        },
        outgoing_source: Polyline {
            nodes: vec![(0., 112.), (8., 128.)],
        },
        incoming_source: Polyline {
            nodes: vec![(0., 0.), (8., 16.)],
        },
        ..Default::default()
    };
    for filter in [&mut cut.lanes.filter_a, &mut cut.lanes.filter_b] {
        filter.hp_hz = Polyline::constant(20.);
        filter.lp_hz = Polyline::constant(20000.);
    }
    cut.lanes.xfader.nodes = vec![(0., -1.), (7.8, -1.), (8., 1.)];
    let mut blend = cut.clone();
    blend.lanes.xfader.nodes = vec![(0., -1.), (8., 1.)];
    blend.summary.as_mut().unwrap().score = 0.55;
    assert!(crate::experience::terminal_handoff(&cut, &a, &b));
    assert!(!crate::experience::terminal_handoff(&blend, &a, &b));
    let chosen = choose_variant(&ctx, -1., Some(cut.clone()), |floor| {
        assert!(
            floor < 0.55,
            "raw score must not exclude the better audible arrangement"
        );
        Some(blend.clone())
    })
    .unwrap();
    assert_eq!(chosen.lanes.xfader.nodes, blend.lanes.xfader.nodes);
    let chosen = choose_variant(&ctx, -1., Some(blend.clone()), |_| Some(cut)).unwrap();
    assert_eq!(chosen.lanes.xfader.nodes, blend.lanes.xfader.nodes);
    // Equal merit keeps the incumbent; a candidate below an external bound
    // does not displace it either.
    assert!(choose_variant(&ctx, 0.7, None, |_| Some(blend)).is_none());
}
