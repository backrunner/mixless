//! Fit actual routed controls in bounded parallel batches. Each batch uses
//! the last resolved prefix's bound, then reduces strictly in input order.
//! Recheck that bound during reduction, preserving serial pruning exactly.
use super::{Frame, Shape};
use crate::PlanContext;
use mixless_protocol::MixPlan;

pub(super) struct Candidate {
    pub shape: Shape,
    pub base: f32,
    pub lower_cost: f32,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn choose(
    plan: &MixPlan,
    ctx: &PlanContext<'_>,
    frames: &[Frame],
    reference: [f32; 2],
    n: f32,
    manual: bool,
    candidates: &[Candidate],
    duration_reward: &(impl Fn(f32) -> f32 + Sync),
) -> Option<(MixPlan, f32)> {
    let evidence = crate::evidence::Prepared::new(ctx, true);
    let prepared = super::render::Prepared::new(plan, ctx, n);
    let mut best = None;
    let mut best_merit = f32::INFINITY;
    let evaluate = |candidate: &Candidate| {
        let mut rendered = plan.clone();
        if super::render::apply_prepared(&mut rendered, frames, &candidate.shape, &prepared)
            .is_none()
            || !super::arrival::preserved(&rendered, ctx, reference)
            || !crate::foreground::preserved(&rendered, ctx)
            || !crate::experience::genuine(&rendered, ctx)
        {
            return None;
        }
        let overlap = crate::experience::overlap_with_reference(
            &rendered,
            ctx.outgoing,
            ctx.incoming,
            reference,
        );
        if !manual && overlap < 1. {
            return None;
        }
        let deduction = duration_reward(n) - duration_reward(overlap);
        let cost = candidate.base + deduction.max(0.);
        let merit = cost
            + crate::foreground::uncertainty(&rendered, ctx)
            + crate::experience::arrival_debt(&rendered, ctx.incoming)
            + if manual {
                0.
            } else {
                crate::energy::loss_cost(&rendered, ctx.outgoing, ctx.incoming)
            };
        Some((merit, rendered, cost))
    };
    let max_batch = if cfg!(feature = "trace") {
        1
    } else {
        2 * std::thread::available_parallelism().map_or(4, |n| n.get())
    };
    let mut start = 0;
    let mut batch = 1;
    while start < candidates.len() {
        let end = (start + batch).min(candidates.len());
        let pending: Vec<_> = candidates[start..end]
            .iter()
            .filter(|c| c.lower_cost < best_merit - 1e-4)
            .collect();
        // Two candidates share one evidence scope to amortize worker setup.
        let run = |i: usize| {
            let part = &pending[i * 2..(i * 2 + 2).min(pending.len())];
            let run = || {
                part.iter()
                    .map(|c| (c.lower_cost, evaluate(c)))
                    .collect::<Vec<_>>()
            };
            if pending.len() <= 2 {
                run()
            } else {
                evidence.run(run)
            }
        };
        let results = if pending.len() <= 2 {
            if pending.is_empty() {
                vec![]
            } else {
                vec![run(0)]
            }
        } else {
            crate::order::parallel::map(pending.len().div_ceil(2), run)
        };
        for (lower_cost, result) in results.into_iter().flatten() {
            if lower_cost >= best_merit - 1e-4 {
                continue;
            }
            if let Some((merit, rendered, cost)) = result {
                if merit < best_merit - 1e-4 {
                    best = Some((rendered, cost));
                    best_merit = merit;
                }
            }
        }
        start = end;
        batch = (batch * 2).min(max_batch);
    }
    best
}
