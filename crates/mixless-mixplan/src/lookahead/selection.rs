//! Different routes can share every source endpoint while sounding different.
//! Keep the existing native/held alternatives through musical ranking. Only
//! their future forecasts may be deduplicated, by exact carried state.
use super::carried_offset;
use crate::PlanContext;
use mixless_protocol::MixPlan;

pub(super) fn choose(
    ctx: &PlanContext<'_>,
    mut candidates: Vec<MixPlan>,
    alternatives: [MixPlan; 2],
    mut prepare: impl FnMut(&mut MixPlan),
    forecast: impl Fn(&MixPlan) -> f32 + Sync,
) -> MixPlan {
    candidates.extend(alternatives);
    for p in &mut candidates {
        prepare(p);
    }
    let mut best = candidates.remove(0);
    let current = |p: &MixPlan| {
        crate::order::audible_cost(p, ctx.outgoing, ctx.incoming, 0., 0.) + carried_offset(p, ctx)
    };
    let key = |p: &MixPlan| {
        [
            crate::established_entry(p, ctx.incoming).to_bits(),
            p.t_end_b.to_bits(),
            p.incoming_offset_end.rate.to_bits(),
            p.incoming_offset_end.pitch_semitones.to_bits(),
        ]
    };
    // Equal carried state implies an identical departure forecast. If every
    // alternative already costs at least as much now, none can improve the
    // result after adding that same nonnegative future cost. This also avoids
    // forecasting a sole route merely because it has duplicate representations.
    if best.failure_reason.is_none()
        && current(&best).is_finite()
        && candidates.iter().all(|p| {
            p.failure_reason.is_some() || (key(p) == key(&best) && current(p) >= current(&best))
        })
    {
        return best;
    }
    if !candidates.is_empty() {
        let mut forecasts = Vec::new();
        let mut best_cost = if best.failure_reason.is_some() {
            f32::INFINITY
        } else {
            let future = forecast(&best);
            forecasts.push((key(&best), future));
            current(&best) + 0.5 * future
        };
        // The first complete cost bounds useful forecasts. Remaining
        // departures are independent; compute each distinct carried state
        // once, then retain the original pruning, reduction and tie order.
        // Speculation is bounded by the existing entrance shortlist.
        let mut pending = Vec::new();
        for p in &candidates {
            if p.failure_reason.is_some() || current(p) >= best_cost {
                continue;
            }
            let k = key(p);
            if !forecasts.iter().any(|(old, _)| *old == k)
                && !pending.iter().any(|(old, _)| *old == k)
            {
                pending.push((k, p));
            }
        }
        let compute = |i: usize| (pending[i].0, forecast(pending[i].1));
        forecasts.extend(if cfg!(feature = "trace") {
            (0..pending.len()).map(compute).collect::<Vec<_>>()
        } else {
            crate::order::parallel::map(pending.len(), compute)
        });
        for candidate in candidates {
            let cost = current(&candidate);
            if cost >= best_cost || candidate.failure_reason.is_some() {
                continue;
            }
            let k = key(&candidate);
            let future = forecasts
                .iter()
                .find(|(old, _)| *old == k)
                .expect("eligible departure was forecast")
                .1;
            let cost = cost + 0.5 * future;
            if cost < best_cost - 0.0001 {
                best = candidate;
                best_cost = cost;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests;
