//! Account for retained plan buffers as well as the fixed-size plan structs.
use mixless_protocol::{AutomationLanes, MixPlan, MixStage, Polyline};

pub(super) fn plans(plans: &Vec<MixPlan>) -> usize {
    plans.capacity() * std::mem::size_of::<MixPlan>() + plans.iter().map(plan).sum::<usize>()
}

fn line(p: &Polyline) -> usize {
    p.nodes.capacity() * std::mem::size_of::<(f32, f32)>()
}

fn plan(p: &MixPlan) -> usize {
    // Exhaustive destructuring makes future heap-owning fields an audit point.
    let MixPlan {
        stem_mix,
        stages,
        summary: _,
        incoming_offset_end: _,
        outgoing_offset: _,
        bar_map: _,
        t_in_a: _,
        t_out_a: _,
        t_in_b: _,
        t_end_b: _,
        source_sample_rates: _,
        clock,
        incoming_source,
        outgoing_source,
        incoming_start_bar: _,
        transition_mode: _,
        master_bpm,
        lanes,
        handoff_bar: _,
        literal_half_double: _,
        failure_reason,
        requires_stems: _,
        send_fx: _,
        insert_a: _,
        incoming_varispeed: _,
    } = p;
    let AutomationLanes {
        xfader,
        gain_a,
        gain_b,
        eq_a,
        eq_b,
        filter_a,
        filter_b,
        fx_send_a,
        fx_send_b,
        rate_a,
        rate_b,
        pitch_a,
        pitch_b,
        loop_a: _,
        loop_b: _,
        scratch_a: _,
        scratch_b: _,
    } = lanes;
    [
        clock,
        incoming_source,
        outgoing_source,
        master_bpm,
        xfader,
        gain_a,
        gain_b,
        &eq_a.low,
        &eq_a.mid,
        &eq_a.high,
        &eq_b.low,
        &eq_b.mid,
        &eq_b.high,
        &filter_a.lp_hz,
        &filter_a.hp_hz,
        &filter_b.lp_hz,
        &filter_b.hp_hz,
        fx_send_a,
        fx_send_b,
        rate_a,
        rate_b,
        pitch_a,
        pitch_b,
    ]
    .into_iter()
    .map(line)
    .sum::<usize>()
        + stem_mix
            .as_ref()
            .map_or(0, |s| s.outgoing.iter().chain(&s.incoming).map(line).sum())
        + stages.capacity() * std::mem::size_of::<MixStage>()
        + stages.iter().map(|s| s.label.capacity()).sum::<usize>()
        + failure_reason.as_ref().map_or(0, String::capacity)
}
