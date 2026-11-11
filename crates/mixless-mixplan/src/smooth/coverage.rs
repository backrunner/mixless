//! Offline search reference, never compiled into the application. Retains the
//! production safety policy and arrangement fitter; this is NOT a global oracle.
use super::*;
use crate::Planner;
use serde_json::json;
use std::{path::Path, time::Instant};

struct Search {
    attempts: usize,
    feasible: usize,
    outs: usize,
    ins: usize,
    entries: Vec<(f32, MixPlan)>,
}

fn search(ctx: &PlanContext<'_>, opts: &PlannerOptions, expanded: bool) -> Search {
    let all_spans = expanded && std::env::var_os("MIXLESS_COVERAGE_ALL_SPANS").is_some();
    let points = if expanded {
        crate::phrasing::audit_points
    } else {
        crate::phrasing::points
    };
    let lengths = if expanded {
        crate::duration::audit_candidates
    } else {
        crate::duration::candidates
    };
    let outs = points(ctx.outgoing, ctx.cues_out, true, opts.earliest_outgoing_sec);
    let ins = points(ctx.incoming, ctx.cues_in, false, 0.);
    let mut result = Search {
        attempts: 0,
        feasible: 0,
        outs: outs.len(),
        ins: ins.len(),
        entries: vec![],
    };
    for input in ins {
        let mut best: Option<(f32, MixPlan)> = None;
        for &out in &outs {
            for mode in [
                TransitionMode::BeatBlend,
                TransitionMode::LoopRoll,
                TransitionMode::PhraseBridge,
            ] {
                let spans = if all_spans {
                    let g = Grid(ctx.outgoing);
                    let max = ((out - g.ceil_bar(opts.earliest_outgoing_sec)) / g.meter())
                        .floor()
                        .clamp(0., 128.) as u16;
                    match mode {
                        TransitionMode::PhraseBridge => (0..=max).collect(),
                        TransitionMode::LoopRoll => (4..=max.min(16)).collect(),
                        _ => (4..=max).collect(),
                    }
                } else {
                    lengths(ctx, opts, out, input, mode)
                };
                for length in spans {
                    result.attempts += 1;
                    // Disable cross-window score pruning. Pair-internal policy
                    // and fitting stay unchanged, including plain/ride choice.
                    if let Some(p) = pair(ctx, opts, out, input, mode, length, -1.) {
                        result.feasible += 1;
                        let merit = crate::experience::merit(&p, ctx.outgoing, ctx.incoming);
                        if best.as_ref().is_none_or(|old| merit > old.0) {
                            best = Some((merit, p));
                        }
                    }
                }
            }
        }
        result.entries.extend(best);
    }
    result
}

fn describe(search: &Search) -> serde_json::Value {
    let best = search.entries.iter().max_by(|a, b| a.0.total_cmp(&b.0));
    json!({
        "out_anchors": search.outs, "in_anchors": search.ins,
        "attempts": search.attempts, "feasible_windows": search.feasible,
        "feasible_entries": search.entries.len(),
        "best_merit": best.map(|p| p.0), "best_plan": best.map(|p| &p.1),
    })
}

fn cost(planner: &Planner, ctx: &PlanContext<'_>, c: &TrackAnalysis, p: &MixPlan) -> f32 {
    if p.failure_reason.is_some() {
        return f32::INFINITY;
    }
    let future = crate::pitch_recovery::departure(
        planner,
        ctx.incoming,
        c,
        p,
        p.incoming_offset_end.pitch_semitones,
        &[],
        &[],
    )
    .map_or(1.5, |next| {
        crate::order::audible_cost(&next, ctx.incoming, c, 0., 0.)
    });
    crate::order::audible_cost(p, ctx.outgoing, ctx.incoming, 0., 0.)
        + crate::lookahead::carried_offset(p, ctx)
        + 0.5 * future
}

// Sensitivity check for a deeper, discounted objective. Future pairs use the
// actual planner with exact carried state, including their own next track.
// This is a fixed-sequence rollout, not an exhaustive tree over future plans.
fn rollout_cost(
    planner: &Planner,
    ctx: &PlanContext<'_>,
    tail: &[TrackAnalysis],
    p: &MixPlan,
) -> f32 {
    if p.failure_reason.is_some() {
        return f32::INFINITY;
    }
    let mut total = crate::order::audible_cost(p, ctx.outgoing, ctx.incoming, 0., 0.)
        + crate::lookahead::carried_offset(p, ctx);
    let mut previous = p.clone();
    let mut outgoing = ctx.incoming;
    let mut discount = 0.5;
    for (i, incoming) in tail.iter().enumerate() {
        let next_planner = Planner::with_options(PlannerOptions {
            outgoing_entry_sec: crate::established_entry(&previous, outgoing),
            earliest_outgoing_sec: previous.t_end_b + previous.incoming_offset_end.rate,
            ..planner.options.clone()
        });
        let next_ctx = PlanContext {
            outgoing,
            incoming,
            cues_out: &[],
            cues_in: &[],
            offset_a: previous.incoming_offset_end,
            offset_b: Default::default(),
        };
        previous = next_planner.plan_with_following(&next_ctx, tail.get(i + 1));
        if previous.failure_reason.is_some() {
            // Explicit infeasibility rather than a numeric success score.
            return f32::INFINITY;
        }
        total += discount * crate::order::audible_cost(&previous, outgoing, incoming, 0., 0.);
        outgoing = incoming;
        discount *= 0.5;
    }
    total
}

fn gap(before: f32, after: f32) -> Option<f32> {
    (before.is_finite() && after.is_finite()).then_some(after - before)
}

/// Supply two to four fresh TrackAnalysis payloads. Optional previous plan
/// reproduces a carried state instead of silently using a cold pair.
#[test]
#[ignore = "offline expanded search: MIXLESS_COVERAGE_INPUT and MIXLESS_COVERAGE_OUTPUT"]
fn compare_search_coverage() {
    let input = std::env::var("MIXLESS_COVERAGE_INPUT").expect("input analyses JSON");
    let output = std::env::var("MIXLESS_COVERAGE_OUTPUT").expect("output JSON");
    let tracks: Vec<TrackAnalysis> =
        serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    assert!((2..=4).contains(&tracks.len()));
    let (a, b) = (&tracks[0], &tracks[1]);
    let previous: Option<MixPlan> = std::env::var("MIXLESS_COVERAGE_PREVIOUS")
        .ok()
        .map(|path| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap());
    if let Some(p) = &previous {
        assert_eq!(p.summary.as_ref().unwrap().pair.1, a.track_id);
    }
    let opts = PlannerOptions {
        harmonic_key_shift: true,
        stem_playback: std::env::var_os("MIXLESS_COVERAGE_NO_STEMS").is_none(),
        outgoing_entry_sec: previous
            .as_ref()
            .map_or(0., |p| crate::established_entry(p, a)),
        earliest_outgoing_sec: previous
            .as_ref()
            .map_or(0., |p| p.t_end_b + p.incoming_offset_end.rate),
        ..Default::default()
    };
    let ctx = PlanContext {
        outgoing: a,
        incoming: b,
        cues_out: &[],
        cues_in: &[],
        offset_a: previous.as_ref().map_or_default(|p| p.incoming_offset_end),
        offset_b: Default::default(),
    };
    let planner = Planner::with_options(opts.clone());
    let session = crate::memo::Session::new(&tracks.iter().collect::<Vec<_>>());
    let _scope = session.enter();
    crate::with_evidence(&ctx, opts.stem_playback, || {
        let start = Instant::now();
        let production = super::plan(&ctx, &opts, None);
        let bounded = search(&ctx, &opts, false);
        let expanded = search(&ctx, &opts, true);
        let supplemental = planner.expanded_plan(&ctx);
        let pair_seconds = start.elapsed().as_secs_f64();
        // Include the same phrase-handoff fallback when comparing scalar merit.
        let baseline = crate::experience::merit(&production, a, b);
        let bounded_merit = bounded.entries.iter().map(|p| p.0).fold(baseline, f32::max);
        let expanded_merit = expanded
            .entries
            .iter()
            .map(|p| p.0)
            .fold(bounded_merit, f32::max);
        let mut result = json!({
            "pair": [a.track_id, b.track_id], "stem_playback": opts.stem_playback,
            "carried_state": previous.is_some(), "pair_seconds": pair_seconds,
            "all_integer_spans": std::env::var_os("MIXLESS_COVERAGE_ALL_SPANS").is_some(),
            "production": production, "production_merit": baseline,
            "supplemental": supplemental,
            "supplemental_feasible": supplemental.failure_reason.is_none(),
            "bounded_unpruned": describe(&bounded), "expanded_unpruned": describe(&expanded),
            "pruning_merit_gap": gap(baseline, bounded_merit),
            "breadth_merit_gap": gap(bounded_merit, expanded_merit),
            "production_feasible": production.failure_reason.is_none(),
            "scope": "evidence anchors and generated spans; unchanged safety, fitting and per-entry best; one following track",
        });
        // Save the completed breadth pass even if a later forecast is interrupted.
        std::fs::write(
            Path::new(&output),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        if let Some(c) = tracks.get(2) {
            let start = Instant::now();
            let production = planner.plan_with_following(&ctx, Some(c));
            let original_cost = cost(&planner, &ctx, c, &production);
            let mut best = production.clone();
            let mut best_cost = original_cost;
            let original_rollout = (tracks.len() == 4)
                .then(|| rollout_cost(&planner, &ctx, &tracks[2..], &production));
            let mut best_rollout = original_rollout;
            let mut deeper_plan = production.clone();
            let mut states = vec![];
            for (_, p) in bounded.entries.into_iter().chain(expanded.entries) {
                let mut p = planner.finish_plan(&ctx, p);
                crate::tempo_recovery::arrange(&planner, &ctx, Some(c), &[], &mut p);
                crate::pitch_recovery::arrange(&planner, &ctx, Some(c), &[], &[], &mut p);
                let k = [
                    crate::established_entry(&p, b).to_bits(),
                    p.t_end_b.to_bits(),
                    p.incoming_offset_end.rate.to_bits(),
                    p.incoming_offset_end.pitch_semitones.to_bits(),
                    p.t_out_a.to_bits(),
                    p.t_in_a.to_bits(),
                ];
                if states.contains(&k) {
                    continue;
                }
                states.push(k);
                let candidate = cost(&planner, &ctx, c, &p);
                if candidate < best_cost - 0.0001 {
                    best = p;
                    best_cost = candidate;
                    if best_rollout.is_some() {
                        let score = rollout_cost(&planner, &ctx, &tracks[2..], &best);
                        if score < best_rollout.unwrap() - 0.0001 {
                            best_rollout = Some(score);
                            deeper_plan = best.clone();
                        }
                    }
                } else if let Some(old) = best_rollout {
                    let score = rollout_cost(&planner, &ctx, &tracks[2..], &p);
                    if score < old - 0.0001 {
                        best_rollout = Some(score);
                        deeper_plan = p;
                    }
                }
            }
            result["lookahead"] = json!({"production": production, "expanded": best,
                "production_cost": original_cost, "expanded_cost": best_cost,
                "cost_gap": gap(best_cost, original_cost), "additional_states": states.len(),
                "seconds": start.elapsed().as_secs_f64()});
            if let (Some(original), Some(expanded)) = (original_rollout, best_rollout) {
                result["two_departure_rollout"] = json!({"production_cost": original,
                    "expanded_cost": expanded, "cost_gap": gap(expanded, original),
                    "production_feasible": original.is_finite(), "expanded_feasible": expanded.is_finite(),
                    "expanded_plan": deeper_plan});
            }
            std::fs::write(&output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        }
        eprintln!("coverage result: {output}");
    });
}

#[test]
fn expanded_anchors_and_spans_preserve_manual_authority_and_bounded_candidates() {
    use mixless_protocol::{Cue, CueKind, PhraseBoundary, SectionLabel as S};
    let mut a = crate::tests::track(1, 120., "8A", S::Outro, 128, 0.8, 0.2);
    a.phrase_boundaries = (1..120)
        .map(|i| PhraseBoundary {
            time_sec: i as f32 * 2.,
            confidence: 0.8,
            novelty: 0.3,
        })
        .collect();
    let b = a.clone();
    let ctx = PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    for out in [false, true] {
        let bounded = points(&a, &[], out, 0.);
        let expanded = crate::phrasing::audit_points(&a, &[], out, 0.);
        assert!(bounded.iter().all(|p| expanded.contains(p)));
        assert!(expanded.len() > bounded.len());
        let cue = Cue {
            index: 0,
            frame: 32 * a.sample_rate as u64,
            kind: if out { CueKind::Out } else { CueKind::In },
            user_set: true,
        };
        assert_eq!(
            points(&a, &[cue.clone()], out, 0.),
            crate::phrasing::audit_points(&a, &[cue], out, 0.)
        );
    }
    for mode in [
        TransitionMode::BeatBlend,
        TransitionMode::LoopRoll,
        TransitionMode::PhraseBridge,
    ] {
        let bounded = crate::duration::candidates(&ctx, &PlannerOptions::default(), 384., 0., mode);
        let expanded =
            crate::duration::audit_candidates(&ctx, &PlannerOptions::default(), 384., 0., mode);
        assert!(bounded.iter().all(|n| expanded.contains(n)));
    }
}
