//! Remove the planner's search pre-roll from a boundary cut. A cut is zero
//! musical bars; the short post-boundary clock only settles transport ownership.
use mixless_protocol::{MixPlan, MixStage, Polyline};

fn crop(line: &mut Polyline, at: f32) {
    if line.nodes.is_empty() {
        return;
    }
    let first = line.sample(at);
    let mut nodes = vec![(0., first)];
    nodes.extend(
        line.nodes
            .iter()
            .filter(|(u, _)| *u > at)
            .map(|(u, v)| (*u - at, *v)),
    );
    line.nodes = nodes;
}

pub(crate) fn compact(p: &mut MixPlan) {
    let at = p.incoming_start_bar;
    let elapsed = p.clock.sample(at);
    crop(&mut p.clock, at);
    for (_, sec) in &mut p.clock.nodes {
        *sec -= elapsed;
    }
    crop(&mut p.outgoing_source, at);
    crop(&mut p.incoming_source, at);
    if p.outgoing_source.nodes.len() == 1 {
        let last = *p.clock.nodes.last().unwrap();
        p.outgoing_source
            .nodes
            .push((last.0, p.t_out_a + last.1 * p.outgoing_offset.rate));
    }
    crop(&mut p.master_bpm, at);
    let l = &mut p.lanes;
    for line in [
        &mut l.xfader,
        &mut l.gain_a,
        &mut l.gain_b,
        &mut l.eq_a.low,
        &mut l.eq_a.mid,
        &mut l.eq_a.high,
        &mut l.eq_b.low,
        &mut l.eq_b.mid,
        &mut l.eq_b.high,
        &mut l.filter_a.hp_hz,
        &mut l.filter_a.lp_hz,
        &mut l.filter_b.hp_hz,
        &mut l.filter_b.lp_hz,
        &mut l.fx_send_a,
        &mut l.fx_send_b,
        &mut l.rate_a,
        &mut l.rate_b,
        &mut l.pitch_a,
        &mut l.pitch_b,
    ] {
        crop(line, at);
    }
    p.t_in_a = p.t_out_a;
    p.incoming_start_bar = 0.;
    p.handoff_bar = Some(0.);
    p.summary.as_mut().unwrap().length_bars = 0;
    p.stages = vec![MixStage {
        start_bar: 0.,
        end_bar: p.clock.nodes.last().unwrap().0,
        label: "Boundary cut".into(),
    }];
}
