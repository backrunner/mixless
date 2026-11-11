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

/// Batches contain independent exit/entry anchors. Each anchor keeps its
/// original mode/length traversal and improves its own snapshot bound. A
/// resolved prefix can change which fallback is attempted: a pruned direct
/// plan may open a prepared plan. Replay stale anchors with the resolved bound,
/// including failures, instead of assuming pruning only removes candidates.
/// Reduce in traversal order so ties never depend on worker completion.
/// Start serially until an incumbent
/// exists (the initial -1 sentinel need not bound every penalized merit).
pub(super) fn single(
    ctx: &PlanContext<'_>,
    options: &PlannerOptions,
    outs: &[f32],
    ins: &[f32],
) -> Scored {
    single_with_batch(
        ctx,
        options,
        outs,
        ins,
        2 * std::thread::available_parallelism().map_or(4, |n| n.get()),
    )
}

fn single_with_batch(
    ctx: &PlanContext<'_>,
    options: &PlannerOptions,
    outs: &[f32],
    ins: &[f32],
    max_batch: usize,
) -> Scored {
    let evidence = crate::evidence::Prepared::new(ctx, options.stem_playback);
    let mut best: Scored = None;
    let count = outs.len() * ins.len();
    let mut start = 0;
    let mut batch = 1;
    while start < count {
        let end = (start + batch).min(count);
        let floor = best.as_ref().map(|p| p.0);
        let job = |index: usize, floor: Option<f32>| {
            let out = outs[index / ins.len()];
            let input = ins[index % ins.len()];
            let mut selected: Scored = None;
            for mode in [
                TransitionMode::BeatBlend,
                TransitionMode::LoopRoll,
                TransitionMode::PhraseBridge,
            ] {
                for length in crate::duration::candidates(ctx, options, out, input, mode) {
                    let bound = selected.as_ref().map(|p| p.0).or(floor);
                    if let Some(plan) =
                        super::pair(ctx, options, out, input, mode, length, bound.unwrap_or(-1.))
                    {
                        let merit = crate::experience::merit(&plan, ctx.outgoing, ctx.incoming);
                        if bound.is_none_or(|old| merit > old) {
                            selected = Some((merit, plan));
                        }
                    }
                }
            }
            selected
        };
        let results = if end - start == 1 {
            vec![job(start, floor)]
        } else {
            crate::order::parallel::map(end - start, |i| evidence.run(|| job(start + i, floor)))
        };
        for (i, result) in results.into_iter().enumerate() {
            let current = best.as_ref().map(|p| p.0);
            let result = if current.map(f32::to_bits) == floor.map(f32::to_bits) {
                result
            } else {
                job(start + i, current)
            };
            if let Some((merit, plan)) = result {
                if best.as_ref().is_none_or(|old| merit > old.0) {
                    best = Some((merit, plan));
                }
            }
        }
        start = end;
        if best.is_some() {
            batch = (batch * 2).min(max_batch.max(1));
        }
    }
    best
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
