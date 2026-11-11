use super::*;
use crate::{MixPlanSummary, StemMix, StrategyId, TrackId};

fn line(nodes: &[(f32, f32)]) -> Polyline {
    Polyline {
        nodes: nodes.to_vec(),
    }
}
fn plan() -> MixPlan {
    let mut p = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (TrackId(1), TrackId(2)),
            strategy: StrategyId::StemSwap,
            score: 1.,
            used_fallback: false,
            length_bars: 8,
        }),
        t_in_a: 20.,
        t_out_a: 36.,
        t_in_b: 10.,
        t_end_b: 34.,
        clock: line(&[(0., 0.), (8., 16.), (12., 24.)]),
        outgoing_source: line(&[(0., 20.), (8., 36.)]),
        incoming_source: line(&[(0., 10.), (12., 34.)]),
        source_sample_rates: [48000; 2],
        ..Default::default()
    };
    p.lanes.xfader = Polyline::constant(0.);
    p.lanes.gain_a = Polyline::constant(0.);
    p.lanes.gain_b = Polyline::constant(0.);
    p.lanes.rate_a = Polyline::constant(1.);
    p.lanes.rate_b = Polyline::constant(1.);
    p.stem_mix = Some(StemMix {
        outgoing: std::array::from_fn(|_| line(&[(0., 1.), (4., 1.), (6., 0.)])),
        incoming: std::array::from_fn(|_| line(&[(0., 0.), (2., 0.), (4., 1.)])),
    });
    p
}

#[test]
fn silent_lead_and_solo_recovery_are_not_part_of_the_exchange() {
    let p = plan();
    let w = p.mix_windows().unwrap();
    assert_eq!(w.outgoing, (24., 32.));
    assert_eq!(w.incoming, (14., 22.));
    assert!(w.incoming.1 < p.t_end_b);
}

#[test]
fn stopped_incoming_and_boundary_cuts_have_no_false_overlap() {
    let mut p = plan();
    p.incoming_start_bar = 8.;
    assert_eq!(
        p.mix_windows().unwrap(),
        MixWindows {
            outgoing: (36., 36.),
            incoming: (10., 10.)
        }
    );
    p.summary.as_mut().unwrap().length_bars = 0;
    assert_eq!(p.mix_windows().unwrap().outgoing, (36., 36.));
}

#[test]
fn outgoing_loop_marks_only_the_source_phrase_actually_repeated() {
    let mut p = plan();
    p.lanes.loop_a = Some(crate::LoopOp {
        start_src_frame: 20 * 48000,
        length_src_frames: 4 * 48000,
        length_bars: 2,
        on_bar: 0.,
        off_bar: 8.,
    });
    assert_eq!(p.mix_windows().unwrap().outgoing, (20., 24.));
}

#[test]
fn incoming_loop_release_and_variable_clock_keep_source_coordinates() {
    let mut p = plan();
    p.lanes.loop_b = Some(crate::LoopOp {
        start_src_frame: 10 * 48000,
        length_src_frames: 4 * 48000,
        length_bars: 2,
        on_bar: 0.,
        off_bar: 4.,
    });
    assert_eq!(p.mix_windows().unwrap().incoming, (10., 18.));
    p.lanes.loop_b = None;
    p.incoming_source.nodes.clear();
    p.clock = line(&[(0., 0.), (4., 8.), (8., 12.), (12., 16.)]);
    p.lanes.rate_b = line(&[(0., 1.), (4., 1.5), (8., 1.)]);
    assert_eq!(p.mix_windows().unwrap().incoming, (14.5, 22.75));
}
