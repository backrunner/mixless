use super::*;
use mixless_protocol::{
    Cue, CueKind, LoopOp, MixPlanSummary, Polyline, SectionLabel, StrategyId, TrackAnalysis,
};

fn pair(stems: bool) -> (TrackAnalysis, TrackAnalysis, MixPlan) {
    let a = crate::tests::track(1, 120., "8A", SectionLabel::Outro, 64, 0.9, 0.2);
    let mut b = crate::tests::track(2, 120., "8A", SectionLabel::Intro, 64, 0.9, 0.2);
    for bar in &mut b.bars {
        bar.vocal_presence = 0.9;
    }
    if stems {
        b = crate::stem_regressions::with_stems(b);
        for f in &mut b.stems.as_mut().unwrap().frames {
            f.vocal_activity = 0.9;
            f.rms[0] = 0.1;
        }
    }
    let mut p = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: StrategyId::PhraseBlend,
            score: 0.8,
            used_fallback: false,
            length_bars: 16,
        }),
        t_out_a: 32.,
        t_end_b: 32.,
        outgoing_source: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        incoming_source: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        ..Default::default()
    };
    p.lanes.xfader.nodes = vec![(0., -1.), (16., 1.)];
    p.lanes.gain_a = Polyline::constant(0.);
    p.lanes.gain_b = Polyline::constant(0.);
    for filter in [&mut p.lanes.filter_a, &mut p.lanes.filter_b] {
        filter.lp_hz = Polyline::constant(20000.);
        filter.hp_hz = Polyline::constant(20.);
    }
    (a, b, p)
}

fn context<'a>(a: &'a TrackAnalysis, b: &'a TrackAnalysis, cues: &'a [Cue]) -> PlanContext<'a> {
    PlanContext {
        outgoing: a,
        incoming: b,
        cues_out: cues,
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    }
}

#[test]
fn an_outgoing_cue_or_loop_does_not_authorize_revealing_half_an_incoming_word() {
    let (a, b, p) = pair(true);
    assert!(!preserved(&p, &context(&a, &b, &[])));
    let cue = [Cue {
        index: 0,
        frame: 32 * 48000,
        kind: CueKind::Out,
        user_set: true,
    }];
    assert!(
        !preserved(&p, &context(&a, &b, &cue)),
        "OUT only authorizes the outgoing side"
    );
    let mut looped = p;
    looped.lanes.loop_a = Some(LoopOp {
        start_src_frame: 0,
        length_bars: 1,
        on_bar: 0.,
        off_bar: 16.,
        length_src_frames: 96000,
    });
    assert!(
        !preserved(&looped, &context(&a, &b, &[])),
        "the incoming source is not looped"
    );
}

#[test]
fn missing_stems_keep_coarse_incoming_vocal_protection() {
    let (a, mut b, p) = pair(false);
    assert!(
        !preserved(&p, &context(&a, &b, &[])),
        "missing stems must not make a sung entrance safe"
    );
    for bar in &mut b.bars {
        bar.vocal_presence = 0.;
        bar.vocal_confidence = Some(0.);
    }
    assert!(
        preserved(&p, &context(&a, &b, &[])),
        "an instrumental fade remains available"
    );
}

#[test]
fn outgoing_manual_cues_still_authorize_the_outgoing_release() {
    let (mut a, mut b, p) = pair(true);
    a = crate::stem_regressions::with_stems(a);
    for f in &mut b.stems.as_mut().unwrap().frames {
        f.vocal_activity = 0.;
        f.rms[0] = 0.;
    }
    assert!(!preserved(&p, &context(&a, &b, &[])));
    let cue = [Cue {
        index: 0,
        frame: 32 * 48000,
        kind: CueKind::Out,
        user_set: true,
    }];
    assert!(preserved(&p, &context(&a, &b, &cue)));
}

#[test]
fn a_manual_in_still_authorizes_its_own_cut_boundary() {
    let (a, b, mut p) = pair(true);
    p.summary.as_mut().unwrap().length_bars = 1;
    p.t_in_b = 8.;
    assert!(!preserved(&p, &context(&a, &b, &[])));
    let cue = [Cue {
        index: 0,
        frame: 8 * 48000,
        kind: CueKind::In,
        user_set: true,
    }];
    let mut ctx = context(&a, &b, &[]);
    ctx.cues_in = &cue;
    assert!(preserved(&p, &ctx));
}
