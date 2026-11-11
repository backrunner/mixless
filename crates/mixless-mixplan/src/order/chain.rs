//! Check the selected loop with the same carried entry/rate/key state as AUTO.
//! Pairwise costs cannot see a late incoming cue that leaves the next pair stuck.
use super::*;
use crate::{short_handoff, PlanContext};
use mixless_protocol::{PerformanceOffset, StrategyId};

#[derive(Clone, Copy)]
struct Step {
    emergency: bool,
    rushed: bool,
    cost: f32,
    offset: PerformanceOffset,
    entry: f32,
    earliest: f32,
}

fn step(
    tracks: [&TrackAnalysis; 3],
    options: &PlannerOptions,
    offset: PerformanceOffset,
    entry: f32,
    earliest: f32,
) -> Step {
    let [a, b, following] = tracks;
    let ctx = PlanContext {
        outgoing: a,
        incoming: b,
        cues_out: &[],
        cues_in: &[],
        offset_a: offset,
        offset_b: PerformanceOffset::identity(),
    };
    let planner = Planner::with_options(PlannerOptions {
        outgoing_entry_sec: entry,
        earliest_outgoing_sec: earliest,
        strategy: None,
        ..options.clone()
    });
    let mut plan = planner.plan_with_following(&ctx, Some(following));
    let unavailable = plan.failure_reason.is_some();
    if unavailable {
        plan = short_handoff(&ctx, earliest);
    }
    #[cfg(feature = "audit")]
    super::audit::write(
        &format!(
            "chain-{}-{}-{}-{:x}-{:x}-{:x}-{:x}",
            a.track_id.0,
            b.track_id.0,
            following.track_id.0,
            entry.to_bits(),
            earliest.to_bits(),
            offset.rate.to_bits(),
            offset.pitch_semitones.to_bits()
        ),
        || serde_json::json!({"plan":plan,"entry":entry,"earliest":earliest,"offset":offset}),
    );
    Step {
        rushed: crate::drops::brief(a, crate::experience::foreground_exit(&plan), entry) > 0.1,
        emergency: unavailable
            || plan.failure_reason.is_some()
            || crate::experience::needs_repair(&plan, a, b)
            || plan.summary.as_ref().is_none_or(|s| {
                s.used_fallback && s.strategy == StrategyId::DryCut && s.length_bars <= 1
            }),
        cost: super::audible_cost(
            &plan,
            a,
            b,
            super::transition_cost(a, b),
            super::energy_cost(a, b),
        ),
        offset: plan.incoming_offset_end,
        entry: crate::established_entry(&plan, b),
        earliest: plan.t_end_b + plan.incoming_offset_end.rate,
    }
}

pub(super) fn repair(
    tracks: &[&TrackAnalysis],
    options: &PlannerOptions,
    costs: &mut [Vec<f32>],
    mut order: Vec<usize>,
    advance: &(impl Fn() + Sync),
    active: &impl Fn() -> bool,
) -> Option<Vec<usize>> {
    let mut best: Option<(usize, f32, Vec<usize>)> = None;
    // Repair rounds often keep part of the chain. Reuse a forecast only when
    // all three tracks and every bit of its carried entry/rate/key match.
    // This cache lives for this immutable request and is bounded by 4*(n+1).
    let mut cache = std::collections::HashMap::new();
    // Bounded background work: at most four complete simulations for small
    // sets. Large-library ordering keeps its existing candidate budget.
    for round in 0..4 {
        let _ = round;
        let mut offset = PerformanceOffset::identity();
        let (mut entry, mut earliest) = (0f32, 0f32);
        let mut observed_costs = Vec::with_capacity(order.len() + 1);
        let mut failed = Vec::new();
        let mut rushed = Vec::new();
        let mut corrections = Vec::new();
        // Revisit the first departure with the closing transition's actual
        // entry. A clean cold start does not prove a clean repeating set.
        for i in 0..=order.len() {
            if !active() {
                return None;
            }
            let (from, to) = (order[i % order.len()], order[(i + 1) % order.len()]);
            let following = order[(i + 2) % order.len()];
            let key = (
                from,
                to,
                following,
                [
                    entry.to_bits(),
                    earliest.to_bits(),
                    offset.rate.to_bits(),
                    offset.pitch_semitones.to_bits(),
                ],
            );
            let step = *cache.entry(key).or_insert_with(|| {
                step(
                    [tracks[from], tracks[to], tracks[following]],
                    options,
                    offset,
                    entry,
                    earliest,
                )
            });
            if step.emergency {
                failed.push((from, to));
            }
            if step.rushed {
                rushed.push((from, to));
            }
            if step.cost > costs[from][to] + 0.02 {
                corrections.push((from, to, step.cost));
            }
            observed_costs.push(step.cost);
            offset = step.offset;
            entry = step.entry;
            earliest = step.earliest;
            advance();
        }
        // The final step replaces the cold first departure with its real
        // closing context. Charging both gives whichever edge opens the list
        // twice the weight, so a mere rotation can beat a better complete set.
        let total = cycle_cost(&observed_costs);
        failed.sort_unstable();
        failed.dedup();
        rushed.sort_unstable();
        rushed.dedup();
        if best
            .as_ref()
            .is_none_or(|b| failed.len() < b.0 || (failed.len() == b.0 && total < b.1))
        {
            best = Some((failed.len(), total, order.clone()));
        }
        #[cfg(feature = "audit")]
        super::audit::write(
            &format!("round-{round}"),
            || serde_json::json!({"order":order,"failed":failed,"rushed":rushed,"corrections":corrections,"total":total,"best":best}),
        );
        if failed.is_empty() && rushed.is_empty() && corrections.is_empty() {
            break;
        }
        for (from, to, cost) in corrections {
            costs[from][to] = costs[from][to].max(cost);
        }
        for (from, to) in failed {
            costs[from][to] += 1.;
        }
        // A feasible edge can still consume the incoming track's foreground
        // and leave it rushing into its departure. Feed the carried-state
        // result back into ordering rather than repairing only emergencies.
        for (from, to) in rushed {
            costs[from][to] += 0.2;
        }
        order = super::order_by_with_progress(costs, &|| {});
    }
    active().then(|| best.map_or(order, |b| b.2))
}

fn cycle_cost(steps: &[f32]) -> f32 {
    steps.iter().skip(1).sum()
}

#[cfg(test)]
mod tests {
    use super::cycle_cost;

    #[test]
    fn closing_context_replaces_the_cold_first_edge_without_rewarding_rotations() {
        let good = [0.9, 0.7, 0.8, 0.9];
        let worse = [0.1, 0.8, 0.9, 0.9];
        assert!(good.iter().sum::<f32>() > worse.iter().sum::<f32>());
        assert!(cycle_cost(&good) < cycle_cost(&worse));
        for rotation in [[0.1, 0.8, 0.9, 0.7], [1.4, 0.9, 0.7, 0.8]] {
            assert!((cycle_cost(&rotation) - cycle_cost(&good)).abs() < 1e-6);
        }
    }
}
