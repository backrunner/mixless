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
        let tracks: Vec<_> = [Some(ctx.outgoing), Some(ctx.incoming), following]
            .into_iter()
            .flatten()
            .collect();
        let session = crate::memo::Session::new(&tracks);
        let _scope = session.enter();
        let mut plan = self.anticipate(ctx, following, incoming_exit_cues, following_entry_cues);
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

    fn anticipate(
        &self,
        ctx: &PlanContext<'_>,
        following: Option<&TrackAnalysis>,
        exit_cues: &[Cue],
        entry_cues: &[Cue],
    ) -> MixPlan {
        let Some(next) =
            following.filter(|t| t.tempo.global_bpm.is_finite() && t.tempo.global_bpm > 0.)
        else {
            let mut plan = self.plan_next(ctx);
            if plan.failure_reason.is_some()
                && self.options.smooth
                && self.options.strategy.is_none()
            {
                let expanded = self.expanded_plan(ctx);
                if expanded.failure_reason.is_none() {
                    plan = expanded;
                }
            }
            crate::tempo_recovery::arrange(self, ctx, None, exit_cues, &mut plan);
            return plan;
        };
        if self.options.strategy.is_some() {
            return self.plan_next(ctx);
        }
        let mut native = self.options.clone();
        native.harmonic_key_shift = false;
        // Holding a useful sounding tempo can avoid restoring B only to change
        // it again for C. This path is useful even when harmonic evidence is absent.
        let mut held = self.options.clone();
        held.strategy = Some(mixless_protocol::StrategyId::EnergyHold);
        let native = Planner::with_options(native);
        let held = Planner::with_options(held);
        // These forecasts share inputs but not carried state. Resolve them in
        // the original order so scheduling cannot change equal-score choices.
        // Trace collectors are thread-local; keep offline rejection audits on
        // their calling thread.
        let (first, alternative, held) = if cfg!(feature = "trace")
            || std::thread::available_parallelism().is_ok_and(|n| n.get() < 3)
        {
            (
                self.plan_choices(ctx, true),
                native.plan_next(ctx),
                held.plan_next(ctx),
            )
        } else {
            let mut results = crate::order::parallel::map(3, |i| match i {
                0 => self.plan_choices(ctx, true),
                1 => vec![native.plan_next(ctx)],
                _ => vec![held.plan_next(ctx)],
            });
            let held = results.pop().unwrap().remove(0);
            let alternative = results.pop().unwrap().remove(0);
            (results.pop().unwrap(), alternative, held)
        };
        let mut candidates = first;
        for p in [alternative, held] {
            if !candidates.iter().any(|old| {
                old.t_in_a == p.t_in_a
                    && old.t_out_a == p.t_out_a
                    && old.t_in_b == p.t_in_b
                    && old.t_end_b == p.t_end_b
                    && old.incoming_offset_end == p.incoming_offset_end
            }) {
                candidates.push(p);
            }
        }
        // Each candidate settles its incoming tempo before B→C is forecast,
        // so the comparison sees the state the next pair really starts from.
        for p in &mut candidates {
            crate::tempo_recovery::arrange(self, ctx, Some(next), exit_cues, p);
        }
        // A distinct incoming phrase changes which peak and recovery B can
        // still play. Evaluate B→C from that exact state, without recursive
        // lookahead. Whole-track tempo/key guesses cannot see this restriction.
        let mut best = candidates.remove(0);
        if !candidates.is_empty() {
            let current = |p: &MixPlan| {
                crate::order::audible_cost(p, ctx.outgoing, ctx.incoming, 0., 0.)
                    + carried_offset(p, ctx)
            };
            let key = |p: &MixPlan| {
                [
                    crate::established_entry(p, ctx.incoming).to_bits(),
                    p.t_end_b.to_bits(),
                    p.incoming_offset_end.rate.to_bits(),
                    p.incoming_offset_end.pitch_semitones.to_bits(),
                ]
            };
            let forecast = |p: &MixPlan| {
                crate::pitch_recovery::departure(
                    self,
                    ctx.incoming,
                    next,
                    p,
                    p.incoming_offset_end.pitch_semitones,
                    exit_cues,
                    entry_cues,
                )
                .map_or(1.5, |next_plan| {
                    crate::order::audible_cost(&next_plan, ctx.incoming, next, 0., 0.)
                })
            };
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
        if best.failure_reason.is_some() {
            // Exhaust the already-computed native/held alternatives before
            // resorting to a terminal handoff.
            if let Some(prepared) = self.prepared_handoff(ctx, next) {
                return prepared;
            }
            if self.options.smooth {
                let mut expanded = self.expanded_plan(ctx);
                if expanded.failure_reason.is_none() {
                    crate::tempo_recovery::arrange(self, ctx, Some(next), exit_cues, &mut expanded);
                    return expanded;
                }
            }
            return best;
        }
        best
    }
}

/// What the listener keeps hearing after the handoff: a transposition or a
/// tempo offset the incoming record still carries once the plan, including
/// its solo recovery, has finished, in proportion to how much of the record
/// remains. A candidate that leaves the record as recorded wins a near tie.
pub(crate) fn carried_offset(p: &MixPlan, ctx: &PlanContext<'_>) -> f32 {
    let b = ctx.incoming;
    let remaining = ((b.duration_sec - p.t_end_b) / b.duration_sec).clamp(0., 1.);
    let end = p.incoming_offset_end;
    let pitch = (end.pitch_semitones - ctx.offset_b.pitch_semitones).abs();
    let tempo = (end.rate / ctx.offset_b.rate).ln().abs();
    if !remaining.is_finite() || !pitch.is_finite() || !tempo.is_finite() {
        return 0.;
    }
    remaining * (2. * crate::score::shift_cost(pitch) + 0.4 * tempo)
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
    fn carried_offset_charges_what_remains_audible_after_the_plan() {
        let a = crate::tests::track(1, 120., "8A", S::Outro, 64, 0.8, 0.2);
        let b = crate::tests::track(2, 120., "8A", S::Intro, 64, 0.8, 0.2);
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        let plan = |pitch: f32, rate: f32, end: f32| MixPlan {
            t_end_b: end,
            incoming_offset_end: mixless_protocol::PerformanceOffset {
                rate,
                pitch_semitones: pitch,
            },
            ..Default::default()
        };
        assert_eq!(carried_offset(&plan(0., 1., 16.), &ctx), 0.);
        let semitone = carried_offset(&plan(1., 1., 16.), &ctx);
        assert!((semitone - 0.05 * (1. - 16. / b.duration_sec)).abs() < 1e-6);
        assert_eq!(carried_offset(&plan(-1., 1., 16.), &ctx), semitone);
        // Less of the record left to hear, less to charge.
        assert!(carried_offset(&plan(1., 1., 96.), &ctx) < semitone);
        assert_eq!(carried_offset(&plan(1., 1., b.duration_sec), &ctx), 0.);
        // The second semitone costs far more than the first.
        assert!(carried_offset(&plan(2., 1., 16.), &ctx) > 4. * semitone);
        // A kept six percent costs about half a kept semitone.
        let tempo = carried_offset(&plan(0., 1.06, 16.), &ctx);
        assert!(tempo > 0.4 * semitone && tempo < 0.6 * semitone);
        // An explicit deck setting is the reference, not an offset.
        let manual = PlanContext {
            offset_b: mixless_protocol::PerformanceOffset {
                rate: 1.06,
                pitch_semitones: 1.,
            },
            ..ctx
        };
        assert_eq!(carried_offset(&plan(1., 1.06, 16.), &manual), 0.);
    }
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
