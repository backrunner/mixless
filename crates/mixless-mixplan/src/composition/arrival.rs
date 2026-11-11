//! Judge an incoming peak by the audible arrangement over a complete bar.
//! Continuing outgoing drums or a carried singer can supply part of its weight;
//! a silent lane does not need to finish a nominal fade on the same downbeat.
use crate::PlanContext;
use mixless_protocol::MixPlan;

pub(super) fn preserved(p: &MixPlan, ctx: &PlanContext<'_>, reference: [f32; 2]) -> bool {
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    for peak in crate::drops::peaks(ctx.incoming) {
        if peak.0 < p.t_in_b - 0.05 || peak.0 >= p.incoming_source.sample(n) - 0.05 {
            continue;
        }
        let start = super::bar_at(&p.incoming_source, peak.0);
        let grid = crate::grid::Grid(ctx.incoming);
        let source_end = grid.sec(grid.beat(peak.0) + grid.meter());
        let end = super::bar_at(&p.incoming_source, source_end).min(n);
        let mut power = [0f32; 4];
        for i in 0..16 {
            let u = start + (end - start) * (i as f32 + 0.5) / 16.;
            let raw = 10f32
                .powf(crate::level::unmixed(ctx.incoming, p.incoming_source.sample(u)) / 10.)
                / reference[1];
            let heard = crate::level::deck_power(p, ctx.outgoing, ctx.incoming, u);
            power[0] += raw;
            power[1] += heard[0] / reference[0] + heard[1] / reference[1];
            if let Some((raw, heard)) =
                crate::level::foreground_power(p, ctx.outgoing, ctx.incoming, u)
            {
                power[2] += raw[1] / reference[1];
                power[3] += heard[0] / reference[0] + heard[1] / reference[1];
            }
        }
        // Match the outgoing peak's complete-bar 3 dB budget. A loud kit
        // cannot conceal an otherwise removed incoming foreground either.
        if power[1] < power[0] * 0.5 || (power[2] / 16. > 0.02 && power[3] < power[2] * 0.125) {
            return false;
        }
    }
    true
}
