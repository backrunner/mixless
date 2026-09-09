//! Short, unsynchronized handoff for late AUTO activation and very short songs.
use crate::PlanContext;
use mixless_protocol::*;

pub fn short_handoff(ctx: &PlanContext<'_>, earliest: f32) -> MixPlan {
    let fail = || MixPlan { failure_reason: Some("No transition fits the remaining audio and mix cues".into()), ..Default::default() };
    if !earliest.is_finite() || ctx.cues_out.iter().chain(ctx.cues_in).any(|c| c.user_set && c.kind != CueKind::Hot) {
        return fail();
    }
    let (a, b) = (ctx.outgoing, ctx.incoming);
    if [a.duration_sec, b.duration_sec, ctx.offset_a.rate, ctx.offset_b.rate].iter().any(|v| !v.is_finite() || *v <= 0.) {
        return fail();
    }
    let input = b.sections.iter().find(|s| s.label != SectionLabel::Silence)
        .map_or(0., |s| s.start_sec).max(0.);
    let start = earliest.max(a.duration_sec - 4. * ctx.offset_a.rate);
    let seconds = ((a.duration_sec - start) / ctx.offset_a.rate)
        .min((b.duration_sec - input) / ctx.offset_b.rate * 0.5).min(4.);
    if seconds < 0.01 { return fail(); }
    let line = |nodes: &[(f32, f32)]| Polyline { nodes: nodes.to_vec() };
    let zero = Polyline::constant(0.);
    let eq = EqLane { low: zero.clone(), mid: zero.clone(), high: zero.clone() };
    let filter = FilterLane { lp_hz: Polyline::constant(20000.), hp_hz: Polyline::constant(20.) };
    MixPlan {
        summary: Some(MixPlanSummary { pair: (a.track_id,b.track_id), strategy: StrategyId::FallbackSwapFilter,
            score: 0.25, used_fallback: true, length_bars: 1 }),
        outgoing_offset: ctx.offset_a, incoming_offset_end: ctx.offset_b,
        t_in_a: start, t_out_a: start + seconds * ctx.offset_a.rate,
        t_in_b: input, t_end_b: input + seconds * ctx.offset_b.rate,
        clock: line(&[(0.,0.),(1.,seconds)]),
        master_bpm: Polyline::constant((a.tempo.global_bpm * ctx.offset_a.rate).clamp(20.,400.)),
        transition_mode: Some(TransitionMode::PhraseBridge),
        lanes: AutomationLanes {
            xfader: line(&[(0.,-1.),(0.25,-0.6875),(0.5,0.),(0.75,0.6875),(1.,1.)]),
            gain_a: zero.clone(), gain_b: zero.clone(), eq_a: eq.clone(), eq_b: eq,
            filter_a: FilterLane { lp_hz: line(&[(0.,20000.),(0.65,14000.),(1.,2000.)]), ..filter.clone() },
            filter_b: filter, fx_send_a: zero.clone(), fx_send_b: zero,
            rate_a: Polyline::constant(ctx.offset_a.rate), rate_b: Polyline::constant(ctx.offset_b.rate),
            pitch_a: Polyline::constant(ctx.offset_a.pitch_semitones), pitch_b: Polyline::constant(ctx.offset_b.pitch_semitones),
            ..Default::default()
        },
        ..Default::default()
    }
}
