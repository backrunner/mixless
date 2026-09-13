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
        let Some(next) = following.filter(|t| t.key_confidence >= 0.55 && t.camelot.is_some())
        else {
            return first;
        };
        if !self.options.harmonic_key_shift || ctx.incoming.key_confidence < 0.55 {
            return first;
        }
        let mut native = self.options.clone();
        native.harmonic_key_shift = false;
        let alternative = Planner::with_options(native).plan_next(ctx);
        let merit = |p: &MixPlan| {
            if p.failure_reason.is_some() {
                return f32::NEG_INFINITY;
            }
            let (compatibility, shift) = crate::score::key_match(
                ctx.incoming,
                next,
                p.incoming_offset_end.pitch_semitones,
                0.,
            );
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
        if merit(&alternative) > merit(&first) + 0.0001 {
            alternative
        } else {
            first
        }
    }
}
