//! Bounded one-track anticipation. Future compatibility can select between
//! safe current plans, but can never override their musical safety gates.
use crate::{PlanContext, Planner};
use mixless_protocol::{MixPlan, TrackAnalysis};

impl Planner {
    pub fn plan_with_following(
        &self,
        ctx: &PlanContext<'_>,
        following: Option<&TrackAnalysis>,
    ) -> MixPlan {
        let first = self.plan_next(ctx);
        let Some(next) =
            following.filter(|t| t.tempo.global_bpm.is_finite() && t.tempo.global_bpm > 0.)
        else {
            return first;
        };
        if self.options.strategy.is_some() {
            return first;
        }
        let mut native = self.options.clone();
        native.harmonic_key_shift = false;
        let alternative = Planner::with_options(native).plan_next(ctx);
        // Holding a useful sounding tempo can avoid restoring B only to change
        // it again for C. This path is useful even when harmonic evidence is absent.
        let mut held = self.options.clone();
        held.strategy = Some(mixless_protocol::StrategyId::EnergyHold);
        let held = Planner::with_options(held).plan_next(ctx);
        let merit = |p: &MixPlan| {
            if p.failure_reason.is_some() {
                return f32::NEG_INFINITY;
            }
            let (compatibility, shift) = if self.options.harmonic_key_shift
                && ctx.incoming.key_confidence >= 0.55
                && next.key_confidence >= 0.55
            {
                crate::score::key_match(
                    ctx.incoming,
                    next,
                    p.incoming_offset_end.pitch_semitones,
                    0.,
                )
            } else {
                (0., 0.)
            };
            let ratio = ctx.incoming.tempo.global_bpm * p.incoming_offset_end.rate
                / next.tempo.global_bpm.max(1.);
            let correction = [ratio, ratio * 0.5, ratio * 2.]
                .into_iter()
                .map(|v| (v - 1.).abs())
                .fold(f32::INFINITY, f32::min);
            p.summary.as_ref().map_or(0., |s| s.score) + 0.06 * compatibility
                - 0.025 * shift.abs()
                - 0.10 * ((correction - 0.08).max(0.) / 0.08).min(1.)
        };
        let mut best = first;
        for candidate in [alternative, held] {
            if merit(&candidate) > merit(&best) + 0.0001 {
                best = candidate;
            }
        }
        best
    }
}
