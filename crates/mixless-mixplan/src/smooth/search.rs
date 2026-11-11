//! Entrance shortlisting has an independent score bound for each incoming
//! point. Distribute those lanes, retaining each lane's original exit order.
//! This preserves pruning exactly, unlike splitting a global-bound search.
use crate::{PlanContext, PlannerOptions};
use mixless_protocol::{MixPlan, TransitionMode};

type Scored = Option<(f32, MixPlan)>;

pub(super) fn entrances(
    ctx: &PlanContext<'_>,
    options: &PlannerOptions,
    outs: &[f32],
    ins: &[f32],
) -> (Scored, Vec<Scored>) {
    let evidence = crate::evidence::Prepared::new(ctx, options.stem_playback);
    let results = crate::order::parallel::map(ins.len(), |input_index| {
        evidence.run(|| {
            let mut best: Option<((usize, usize, usize, usize), f32, MixPlan)> = None;
            for (out_index, &out) in outs.iter().enumerate() {
                for (mode_index, mode) in [
                    TransitionMode::BeatBlend,
                    TransitionMode::LoopRoll,
                    TransitionMode::PhraseBridge,
                ]
                .into_iter()
                .enumerate()
                {
                    for (length_index, length) in
                        crate::duration::candidates(ctx, options, out, ins[input_index], mode)
                            .into_iter()
                            .enumerate()
                    {
                        if let Some(plan) = super::pair(
                            ctx,
                            options,
                            out,
                            ins[input_index],
                            mode,
                            length,
                            best.as_ref().map_or(-1., |p| p.1),
                        ) {
                            let merit = crate::experience::merit(&plan, ctx.outgoing, ctx.incoming);
                            if best.as_ref().is_none_or(|old| merit > old.1) {
                                best = Some((
                                    (out_index, input_index, mode_index, length_index),
                                    merit,
                                    plan,
                                ));
                            }
                        }
                    }
                }
            }
            best
        })
    });
    // Equal scores belong to the earliest candidate in the original
    // exit/input/mode/length traversal, not whichever worker finishes first.
    let mut best: Option<&((usize, usize, usize, usize), f32, MixPlan)> = None;
    for p in results.iter().flatten() {
        if best.is_none_or(|old| p.1 > old.1 || p.1 == old.1 && p.0 < old.0) {
            best = Some(p);
        }
    }
    let best = best.map(|p| (p.1, p.2.clone()));
    (
        best,
        results
            .into_iter()
            .map(|p| p.map(|(_, score, plan)| (score, plan)))
            .collect(),
    )
}
