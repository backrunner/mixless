//! Bounded one-track anticipation. Future compatibility can select between
//! safe current plans, but can never override their musical safety gates.
use crate::{PlanContext, Planner};
use mixless_protocol::{Cue, MixPlan, TrackAnalysis};

impl Planner {
    pub fn plan_with_following(
        &self,
        ctx: &PlanContext<'_>,
        following: Option<&TrackAnalysis>,
    ) -> MixPlan {
        self.plan_with_following_cues(ctx, following, &[], &[])
    }

    /// Exit cues belong to B's later departure, not its current entrance.
    pub fn plan_with_following_cues(
        &self,
        ctx: &PlanContext<'_>,
        following: Option<&TrackAnalysis>,
        incoming_exit_cues: &[Cue],
        following_entry_cues: &[Cue],
    ) -> MixPlan {
        let mut plan = self.anticipate(ctx, following);
        crate::pitch_recovery::arrange(
            self,
            ctx,
            following,
            incoming_exit_cues,
            following_entry_cues,
            &mut plan,
        );
        plan
    }

    fn anticipate(&self, ctx: &PlanContext<'_>, following: Option<&TrackAnalysis>) -> MixPlan {
        let first = self.plan_next(ctx);
        let Some(next) =
            following.filter(|t| t.tempo.global_bpm.is_finite() && t.tempo.global_bpm > 0.)
        else {
            return first;
        };
        if self.options.strategy.is_some() {
            return first;
        }
        if first.failure_reason.is_some() {
            // A terminal handoff can prepare the silent incoming deck for C.
            // Otherwise B may enter too late at native speed to reach its
            // first complete-drop exit before a slow tempo ramp has finished.
            return self.prepared_handoff(ctx, next).unwrap_or(first);
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

impl Planner {
    fn prepared_handoff(&self, ctx: &PlanContext<'_>, next: &TrackAnalysis) -> Option<MixPlan> {
        let native = ctx.incoming.tempo.global_bpm;
        let sounding = native * ctx.offset_b.rate;
        let following = [0.5, 1., 2.]
            .into_iter()
            .map(|factor| next.tempo.global_bpm * factor)
            .min_by(|a, b| ((sounding / a).ln().abs()).total_cmp(&(sounding / b).ln().abs()))?;
        let low = (native * 0.9201)
            .max(following * 0.9201)
            .max(sounding * 0.9201);
        let high = (native * 1.0799)
            .min(following * 1.0799)
            .min(sounding * 1.0799);
        if !low.is_finite() || !high.is_finite() || native <= 0. || low > high {
            return None;
        }
        let rate = sounding.clamp(low, high) / native;
        if (rate - ctx.offset_b.rate).abs() < 0.0001 {
            return None;
        }
        let prepared = PlanContext {
            offset_b: mixless_protocol::PerformanceOffset {
                rate,
                ..ctx.offset_b
            },
            ..*ctx
        };
        let mut plan = crate::short_handoff(&prepared, self.options.earliest_outgoing_sec);
        // Only an actual boundary cut has a silent incoming deck throughout
        // the outgoing phrase. Never change an overlapping fallback's clock.
        if plan.failure_reason.is_some()
            || plan.summary.as_ref()?.strategy != mixless_protocol::StrategyId::DryCut
            || plan.incoming_start_bar < plan.summary.as_ref()?.length_bars as f32
        {
            return None;
        }
        plan.stages[0].label = "Complete phrase at prepared tempo".into();
        Some(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::SectionLabel as S;
    #[test]
    fn terminal_entry_can_prepare_the_next_clock_without_touching_the_sung_exit() {
        for scale in [0.8, 1., 1.1] {
            let mut a = crate::tests::track(1, 147. * scale, "8A", S::Verse, 32, 0.8, 0.9);
            for bar in &mut a.bars {
                bar.vocal_presence = 0.9;
                bar.vocal_confidence = Some(0.9);
            }
            let b = crate::tests::track(2, 152. * scale, "3B", S::Intro, 64, 0.8, 0.1);
            let c = crate::tests::track(3, 174. * scale, "3B", S::Intro, 64, 0.8, 0.1);
            let ctx = PlanContext {
                outgoing: &a,
                incoming: &b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            };
            let p = Planner::new().prepared_handoff(&ctx, &c).unwrap();
            assert_eq!(p.t_out_a, a.duration_sec);
            assert!(p.incoming_start_bar >= p.summary.as_ref().unwrap().length_bars as f32);
            assert!((p.incoming_offset_end.rate - 174. * 0.9201 / 152.).abs() < 0.0001);
            assert_eq!(p.lanes.rate_a.sample(0.), 1.);
            assert_eq!(p.incoming_offset_end.pitch_semitones, 0.);
            let far = crate::tests::track(4, 210. * scale, "3B", S::Intro, 64, 0.8, 0.1);
            assert!(Planner::new().prepared_handoff(&ctx, &far).is_none());
        }
    }
}
