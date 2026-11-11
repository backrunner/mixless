//! Solo tempo restoration. A beat-matched handoff can leave the incoming deck
//! at the outgoing tempo, and nothing after the overlap justifies playing the
//! rest of the record that way. The glide is part of the committed clock, so
//! the next pair plans from the tempo actually reached.
use crate::{grid::Grid, PlanContext, Planner};
use mixless_protocol::{Cue, CueKind, MixPlan, MixStage, Polyline, TrackAnalysis};

/// Offset worth keeping towards a following track that needs the same move.
const KEEP: f32 = 0.06;
/// A smaller correction is not worth a gesture.
const MINIMUM: f32 = 0.005;
/// Mean log-tempo slope of the eased glide. The quintic ease peaks at 1.875
/// times its mean, with the same margin the shared-tempo preparation uses.
const MEAN_SLOPE: f32 = crate::smooth::MAX_LOG_TEMPO_PER_SEC / (1.875 * 1.1);

/// The rate the incoming deck should settle at. Native, unless the following
/// track needs this one to move the same way: both decks then meet in the
/// middle, so up to half that gap stays. Recovery never adds movement.
pub(crate) fn target(
    b: &TrackAnalysis,
    held: f32,
    native: f32,
    following: Option<&TrackAnalysis>,
) -> f32 {
    let bpm = b.tempo.global_bpm * native;
    let Some(next) = following
        .map(|t| t.tempo.global_bpm)
        .filter(|next| next.is_finite() && *next > 0. && bpm.is_finite() && bpm > 0.)
    else {
        return native;
    };
    let gap = (next * crate::grid::tempo_factor(next / bpm) / bpm).ln();
    let useful = (0.5 * gap).clamp(-KEEP.ln_1p(), KEEP.ln_1p());
    let offset = (held / native).ln();
    native * useful.clamp(offset.min(0.), offset.max(0.)).exp()
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (10. + t * (-15. + 6. * t))
}

pub(super) fn arrange(
    planner: &Planner,
    ctx: &PlanContext<'_>,
    following: Option<&TrackAnalysis>,
    exit_cues: &[Cue],
    plan: &mut MixPlan,
) {
    let held = plan.incoming_offset_end.rate;
    // Restore only the automatic correction, preserving an explicit tempo.
    let native = ctx.offset_b.rate;
    if !planner.options.smooth
        || planner.options.strategy.is_some()
        || plan.failure_reason.is_some()
        || plan.transition_mode.is_none()
        || !(0.5..=2.).contains(&held)
        || !(0.5..=2.).contains(&native)
    {
        return;
    }
    let b = ctx.incoming;
    let Some(&(end_bar, wall)) = plan.clock.nodes.last() else {
        return;
    };
    // Only extend a deck already settled at the rate it carries forward.
    if plan.lanes.rate_b.nodes.is_empty()
        || (plan.lanes.rate_b.sample(end_bar) - held).abs() > 0.001
    {
        return;
    }
    let g = Grid(b);
    // The closing phrases, and half of what remains before them, stay free
    // for the next departure. A longer correction is completed part-way.
    let mut reserve = g.sec(g.beat(b.duration_sec) - 8. * g.meter());
    // The incoming track's OUT belongs to its next departure. Restoring its
    // speed must not consume that exit or the same eight-bar staging reserve.
    for cue in exit_cues
        .iter()
        .filter(|c| c.user_set && c.kind == CueKind::Out)
    {
        let sec = cue.frame as f32 / b.sample_rate.max(1) as f32;
        if sec > plan.t_end_b {
            reserve = reserve.min(g.sec(g.beat(sec) - 8. * g.meter()));
        }
    }
    // An unlocked deck carries its pitch in its speed. Keeping part of the
    // offset would keep a fraction of a semitone, so it returns all the way.
    let goal = if plan.incoming_varispeed {
        native
    } else {
        target(b, held, native, following)
    };
    let room = (0.5 * (reserve - plan.t_end_b) / held.max(goal)).min(1799. - wall);
    let change = (goal / held)
        .ln()
        .clamp(-room.max(0.) * MEAN_SLOPE, room.max(0.) * MEAN_SLOPE);
    if change.abs() < MINIMUM.ln_1p() {
        return;
    }
    let seconds = change.abs() / MEAN_SLOPE;
    let steps = (seconds * 2.).ceil().max(16.) as usize;
    let bars_per_second = g.bpm(g.beat(plan.t_end_b)) * held / (60. * g.meter());
    let (mut rates, mut clock, mut sources, mut tempo) = (vec![], vec![], vec![], vec![]);
    let (mut source, mut previous) = (0., held);
    for j in 1..=steps {
        let x = j as f32 / steps as f32;
        let rate = held * (change * ease(x)).exp();
        // The rate lane is linear between nodes, so the trapezoid is exact.
        source += 0.5 * (previous + rate) * seconds / steps as f32;
        let u = end_bar + bars_per_second * seconds * x;
        rates.push((u, rate));
        clock.push((u, wall + seconds * x));
        sources.push((u, source));
        tempo.push((u, g.bpm(g.beat(plan.t_end_b + source)) * rate));
        previous = rate;
    }
    // Pin the old endpoint first: otherwise the last overlap node would
    // interpolate through the glide.
    let extend = |line: &mut Polyline, value: f32, points: Vec<(f32, f32)>| {
        line.nodes.retain(|(u, _)| *u < end_bar);
        line.nodes.push((end_bar, value));
        line.nodes.extend(points);
    };
    extend(&mut plan.lanes.rate_b, held, rates);
    if !plan.master_bpm.nodes.is_empty() {
        let value = plan.master_bpm.sample(end_bar);
        extend(&mut plan.master_bpm, value, tempo);
    }
    if let Some(&(u, at)) = plan.incoming_source.nodes.last() {
        let origin = at + (wall - plan.clock.sample(u)) * held;
        let points = sources.iter().map(|(u, s)| (*u, origin + s)).collect();
        extend(&mut plan.incoming_source, origin, points);
    }
    let end = clock.last().map_or(end_bar, |n| n.0);
    plan.clock.nodes.extend(clock);
    plan.stages.push(MixStage {
        start_bar: end_bar,
        end_bar: end,
        label: "Restore tempo".into(),
    });
    plan.t_end_b += source;
    plan.incoming_offset_end.rate = previous;
    if plan.incoming_varispeed {
        plan.incoming_offset_end.pitch_semitones = plan.incoming_pitch_at(end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{PerformanceOffset, SectionLabel as S, TransitionMode};

    fn track(bpm: f32, bars: u32) -> TrackAnalysis {
        crate::tests::track(2, bpm, "8A", S::Drop, bars, 0.8, 0.5)
    }

    /// A finished four-bar overlap that leaves B at `rate`.
    fn held(rate: f32) -> MixPlan {
        let mut p = MixPlan {
            transition_mode: Some(TransitionMode::BeatBlend),
            clock: Polyline {
                nodes: vec![(0., 0.), (4., 8.)],
            },
            incoming_source: Polyline {
                nodes: vec![(0., 0.), (4., 8. * rate)],
            },
            master_bpm: Polyline::constant(120. * rate),
            t_end_b: 8. * rate,
            incoming_offset_end: PerformanceOffset {
                rate,
                pitch_semitones: 0.,
            },
            ..Default::default()
        };
        p.lanes.xfader = Polyline {
            nodes: vec![(0., -1.), (4., 1.)],
        };
        p.lanes.rate_b = Polyline::constant(rate);
        p
    }

    fn restore(b: &TrackAnalysis, next: Option<&TrackAnalysis>, p: &mut MixPlan) {
        arrange(
            &Planner::new(),
            &PlanContext {
                outgoing: b,
                incoming: b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            },
            next,
            &[],
            p,
        );
    }

    #[test]
    fn target_is_native_unless_the_next_track_needs_the_same_move() {
        let b = track(150., 64);
        assert_eq!(target(&b, 1.12, 1., None), 1.);
        // Slower or equal next track: nothing to keep.
        assert_eq!(target(&b, 1.12, 1., Some(&track(145., 64))), 1.);
        assert_eq!(target(&b, 0.9, 1., Some(&track(160., 64))), 1.);
        // Faster next track: half the gap, at most six percent, never more
        // than is already held.
        let near = target(&b, 1.12, 1., Some(&track(159., 64)));
        assert!((near - (159f32 / 150.).sqrt()).abs() < 1e-4);
        assert!((target(&b, 1.12, 1., Some(&track(174., 64))) - 1.06).abs() < 1e-4);
        assert!((target(&b, 1.02, 1., Some(&track(174., 64))) - 1.02).abs() < 1e-5);
        // Half and double time are the same tempo.
        assert!((target(&b, 1.12, 1., Some(&track(75., 64))) - 1.).abs() < 1e-5);
        // An explicit deck tempo is the reference, not the file's.
        assert!((target(&b, 1.1, 1.05, None) - 1.05).abs() < 1e-6);
    }

    #[test]
    fn glide_returns_to_native_within_the_tempo_limit_and_keeps_the_overlap() {
        let b = track(120., 128);
        let mut p = held(1.12);
        let original = p.clone();
        restore(&b, None, &mut p);
        assert!((p.incoming_offset_end.rate - 1.).abs() < 1e-4);
        let end = p.clock.nodes.last().unwrap().0;
        assert!((p.lanes.rate_b.sample(end) - 1.).abs() < 1e-4);
        for i in 0..=64 {
            let u = i as f32 / 16.;
            assert_eq!(p.lanes.rate_b.sample(u), 1.12);
            assert_eq!(p.clock.sample(u), original.clock.sample(u));
            assert_eq!(
                p.incoming_source.sample(u),
                original.incoming_source.sample(u)
            );
        }
        // Every line stays strictly ordered for the engine.
        for line in [&p.clock, &p.incoming_source, &p.lanes.rate_b, &p.master_bpm] {
            assert!(line.nodes.windows(2).all(|n| n[1].0 > n[0].0));
        }
        assert!(p.clock.nodes.windows(2).all(|n| n[1].1 > n[0].1));
        assert!(p.incoming_source.nodes.windows(2).all(|n| n[1].1 > n[0].1));
        let mut source = original.t_end_b;
        for pair in p.lanes.rate_b.nodes.windows(2).filter(|n| n[0].0 >= 4.) {
            let seconds = p.clock.sample(pair[1].0) - p.clock.sample(pair[0].0);
            let slope = (pair[1].1 / pair[0].1).ln().abs() / seconds;
            assert!(slope <= crate::smooth::MAX_LOG_TEMPO_PER_SEC * 1.02);
            source += 0.5 * (pair[0].1 + pair[1].1) * seconds;
        }
        // The source clock, its curve and the carried end agree.
        assert!((p.t_end_b - source).abs() < 0.01);
        assert!((p.incoming_source.nodes.last().unwrap().1 - p.t_end_b).abs() < 0.01);
        assert!((p.master_bpm.sample(end) - 120.).abs() < 0.05);
        let stage = p.stages.last().unwrap();
        assert_eq!(stage.label, "Restore tempo");
        assert_eq!((stage.start_bar, stage.end_bar), (4., end));
        // Already settled: a second pass has nothing to do.
        let settled = p.clone();
        restore(&b, None, &mut p);
        assert_eq!(p.clock.nodes, settled.clock.nodes);
    }

    #[test]
    fn keeps_part_of_the_offset_for_a_faster_following_track() {
        let b = track(120., 128);
        let mut p = held(1.12);
        restore(&b, Some(&track(139.2, 64)), &mut p);
        assert!((p.incoming_offset_end.rate - 1.06).abs() < 1e-4);
        let mut slower = held(0.9);
        restore(&b, Some(&track(139.2, 64)), &mut slower);
        assert!((slower.incoming_offset_end.rate - 1.).abs() < 1e-4);
    }

    #[test]
    fn a_short_remainder_gets_a_partial_correction_and_keeps_its_ending() {
        let b = track(120., 32);
        let g = Grid(&b);
        let reserve = g.sec(g.beat(b.duration_sec) - 8. * g.meter());
        let mut p = held(1.12);
        let start = p.t_end_b;
        restore(&b, None, &mut p);
        assert!(p.incoming_offset_end.rate > 1.01 && p.incoming_offset_end.rate < 1.12);
        assert!(p.t_end_b <= start + 0.5 * (reserve - start) + 0.01);
        // With no room at all the plan is left alone.
        let mut late = held(1.12);
        late.t_end_b = reserve;
        let untouched = late.clone();
        restore(&b, None, &mut late);
        assert_eq!(late.clock.nodes, untouched.clock.nodes);
        assert_eq!(late.incoming_offset_end, untouched.incoming_offset_end);
    }

    #[test]
    fn planned_transitions_hand_the_next_pair_a_record_at_its_own_tempo() {
        let mut restored = 0;
        // Near tempos glide back inside the overlap; the wider gaps hold the
        // incoming deck 8% off and need the solo glide.
        for bpm in [120., 127., 131., 136., 110.] {
            let a = crate::tests::track(1, 120., "8A", S::Outro, 96, 0.9, 0.2);
            let b = crate::tests::track(2, bpm, "8A", S::Intro, 128, 0.9, 0.2);
            let ctx = PlanContext {
                outgoing: &a,
                incoming: &b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            };
            let plain = Planner::new().plan_next(&ctx);
            let p = Planner::new().plan_with_following(&ctx, None);
            assert!(p.failure_reason.is_none(), "{bpm}: {:?}", p.failure_reason);
            assert!(
                (p.incoming_offset_end.rate - 1.).abs() < MINIMUM,
                "{bpm}: settles at {}",
                p.incoming_offset_end.rate
            );
            // The overlap itself is the plan the pair would get anyway.
            let n = p.summary.as_ref().unwrap().length_bars as f32;
            assert_eq!(p.t_out_a, plain.t_out_a);
            assert_eq!(p.lanes.rate_b.sample(n), plain.lanes.rate_b.sample(n));
            if (plain.incoming_offset_end.rate - 1.).abs() >= MINIMUM {
                restored += 1;
                assert_eq!(p.stages.last().unwrap().label, "Restore tempo");
                assert!(p.t_end_b > plain.t_end_b && p.t_end_b < b.duration_sec);
                // What the engine requires of every plan line.
                for line in [&p.clock, &p.lanes.rate_b] {
                    assert!(line.nodes.windows(2).all(|n| n[1].0 > n[0].0));
                }
                assert!(p.clock.nodes.windows(2).all(|n| n[1].1 > n[0].1));
                assert!(p
                    .stages
                    .windows(2)
                    .all(|s| s[1].start_bar >= s[0].end_bar - 0.001));
            }
        }
        assert_eq!(restored, 3);
    }

    #[test]
    fn leaves_failed_explicit_and_unsettled_plans_alone() {
        let b = track(120., 128);
        let mut failed = held(1.12);
        failed.failure_reason = Some("no window".into());
        let mut moving = held(1.12);
        moving.lanes.rate_b = Polyline {
            nodes: vec![(0., 1.), (4., 1.05)],
        };
        for mut p in [failed, moving, held(1.003)] {
            let before = p.clone();
            restore(&b, None, &mut p);
            assert_eq!(p.clock.nodes, before.clock.nodes);
            assert_eq!(p.t_end_b, before.t_end_b);
        }
        let mut explicit = held(1.12);
        let before = explicit.clone();
        arrange(
            &Planner::with_options(crate::PlannerOptions {
                strategy: Some(mixless_protocol::StrategyId::EnergyHold),
                ..Default::default()
            }),
            &PlanContext {
                outgoing: &b,
                incoming: &b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            },
            None,
            &[],
            &mut explicit,
        );
        assert_eq!(explicit.clock.nodes, before.clock.nodes);
    }

    #[test]
    fn solo_tempo_restoration_does_not_consume_the_incoming_tracks_manual_exit() {
        use mixless_protocol::{Cue, CueKind};
        let a = crate::tests::track(1, 120., "8A", S::Outro, 96, 0.9, 0.2);
        let b = track(136., 128);
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        let planner = Planner::new();
        let plain = planner.plan_next(&ctx);
        assert!(plain.failure_reason.is_none());
        assert!((plain.incoming_offset_end.rate - 1.).abs() > MINIMUM);
        let exit = plain.t_end_b + 2. * plain.incoming_offset_end.rate;
        let cue = Cue {
            index: 7,
            frame: (exit * b.sample_rate as f32).round() as u64,
            kind: CueKind::Out,
            user_set: true,
        };
        let recovered =
            planner.plan_with_following_cues(&ctx, None, std::slice::from_ref(&cue), &[]);
        assert!(
            recovered.t_end_b < exit,
            "recovery ends at {}, after manual OUT {exit}",
            recovered.t_end_b
        );
        // An automatic suggestion or a Hot cue does not constrain departure.
        for cue in [
            Cue {
                user_set: false,
                ..cue.clone()
            },
            Cue {
                kind: CueKind::Hot,
                ..cue.clone()
            },
        ] {
            let p = planner.plan_with_following_cues(&ctx, None, &[cue], &[]);
            assert!(p.t_end_b > exit);
            assert!((p.incoming_offset_end.rate - 1.).abs() < MINIMUM);
        }
        // A later explicit exit leaves enough room for the normal recovery.
        let late = Cue {
            frame: (b.duration_sec * b.sample_rate as f32) as u64,
            ..cue
        };
        let p = planner.plan_with_following_cues(&ctx, None, &[late], &[]);
        assert!((p.incoming_offset_end.rate - 1.).abs() < MINIMUM);
    }
}
