//! Solo key restoration is part of the committed clock, so the next pair sees
//! the pitch actually reached. Never change pitch while the records overlap.
use crate::{PlanContext, Planner, cue_policy::cue_choices, grid::Grid};
use mixless_protocol::{Cue, MixPlan, MixStage, PerformanceOffset, Polyline, TrackAnalysis};

#[cfg(test)]
mod tests;
mod window;

pub(super) fn arrange(
    planner: &Planner,
    ctx: &PlanContext<'_>,
    following: Option<&TrackAnalysis>,
    exit_cues: &[Cue],
    entry_cues: &[Cue],
    plan: &mut MixPlan,
) {
    let held = plan.incoming_offset_end.pitch_semitones;
    // Restore only the automatic correction, preserving an explicitly tuned B.
    let native = ctx.offset_b.pitch_semitones;
    let distance = (held - native).abs();
    if !planner.options.harmonic_key_shift
        || !planner.options.smooth
        || planner.options.strategy.is_some()
        || plan.failure_reason.is_some()
        || plan.transition_mode.is_none()
        || !(0.5..=6.).contains(&distance)
        || plan.insert_a.is_some()
    {
        return;
    }
    let b = ctx.incoming;
    let g = Grid(b);
    let rate = plan.incoming_offset_end.rate;
    let Some(&(end_bar, _)) = plan.clock.nodes.last() else {
        return;
    };
    // Only append to a settled incoming deck. Existing overlap envelopes and
    // tempo recovery are preserved byte-for-byte.
    if (plan.lanes.xfader.sample(end_bar) - 1.).abs() > 0.001
        || plan.lanes.fx_send_a.sample(end_bar) > 0.001
        || plan.lanes.fx_send_b.sample(end_bar) > 0.001
        || (plan.lanes.rate_b.sample(end_bar) - rate).abs() > 0.001
        || [
            &plan.lanes.eq_b.low,
            &plan.lanes.eq_b.mid,
            &plan.lanes.eq_b.high,
        ]
        .iter()
        .any(|l| l.sample(end_bar).abs() > 0.001)
        || plan.lanes.filter_b.lp_hz.sample(end_bar) < 19999.
        || plan.lanes.filter_b.hp_hz.sample(end_bar) > 20.01
        || plan.stem_mix.as_ref().is_some_and(|s| {
            s.incoming
                .iter()
                .any(|l| (l.sample(end_bar) - 1.).abs() > 0.001)
        })
    {
        return;
    }
    let following = following.filter(|t| t.duration_sec.is_finite() && t.duration_sec > 0.);
    let future =
        following.and_then(|c| departure(planner, b, c, plan, held, exit_cues, entry_cues));
    // Failed anticipation is not permission to consume the next departure.
    if following.is_some() && future.is_none() {
        return;
    }
    let reserve = g.sec(g.beat(b.duration_sec) - 8. * g.meter());
    // The preferred next exit is a candidate, not a published constraint yet.
    // A later equally good departure may make room for restoration. Explicit
    // cues remain hard constraints in the second planning pass below.
    let mut latest = reserve;
    latest = latest.min(plan.t_end_b + rate * (1800. - plan.duration_sec()).max(0.));
    if following.is_none() {
        for cue in exit_cues
            .iter()
            .filter(|c| c.user_set && c.kind == mixless_protocol::CueKind::Out)
        {
            let sec = cue.frame as f32 / b.sample_rate as f32;
            if sec > plan.t_end_b {
                latest = latest.min(g.sec(g.beat(sec) - 8. * g.meter()));
            }
        }
    }
    let guard = g.meter() * 60. / g.bpm(g.beat(plan.t_end_b)) / rate;
    let earliest = plan.t_end_b + rate * (guard + plan.send_fx.map_or(0., |fx| fx.tail_seconds));
    // Native is preferred; a partial restoration can avoid making the next
    // record move much farther. The cost is in squared semitones, with the
    // current record weighted more than the one-track forecast.
    let cost = |pitch: f32| {
        let future_shift = future
            .as_ref()
            .zip(following)
            .and_then(|(p, c)| {
                (p.summary.as_ref()?.length_bars > 0).then_some(())?;
                let (.., shift, reliable) = crate::score::overlap_key_match(
                    b,
                    c,
                    (p.t_in_a, p.t_out_a),
                    (p.t_in_b, p.t_end_b),
                    pitch,
                    0.,
                )
                .unwrap_or_else(|| {
                    let (fit, shift) = crate::score::key_match(b, c, pitch, 0.);
                    (
                        fit,
                        shift,
                        b.key_confidence >= 0.55 && c.key_confidence >= 0.55,
                    )
                });
                reliable.then_some(shift)
            })
            .unwrap_or(0.);
        (pitch - native).powi(2) + 0.6 * future_shift.powi(2)
    };
    let held_cost = cost(held);
    let mut best = None;
    for step in 0..distance.ceil() as usize {
        let target = native + (held - native).signum() * step as f32;
        let movement = (held - target).abs();
        if movement < 0.5 {
            continue;
        }
        let benefit = held_cost - cost(target) - 0.15 * movement * movement;
        if benefit <= 0.1 {
            continue;
        }
        let Some(window) = window::find(b, earliest, latest, rate, movement) else {
            continue;
        };
        let merit = benefit - 0.5 * window.risk - 0.002 * (window.start - earliest) / rate;
        if best.as_ref().is_none_or(|(old, _, _)| merit > *old) {
            best = Some((merit, target, window));
        }
    }
    let Some((merit, target, window)) = best else {
        return;
    };
    if let Some(c) = following {
        let mut recovered = plan.clone();
        recovered.t_end_b = window.end;
        let Some(next) = departure(planner, b, c, &recovered, target, exit_cues, entry_cues) else {
            return;
        };
        // Verify the selected pitch against a real next-pair plan. Do not turn
        // a working overlap into a hard cut just to get closer to native key.
        if next.t_in_a < window.end + rate
            || future.as_ref().is_some_and(|old| {
                let actual_benefit = distance.powi(2)
                    + 0.6 * old.incoming_offset_end.pitch_semitones.powi(2)
                    - (target - native).powi(2)
                    - 0.6 * next.incoming_offset_end.pitch_semitones.powi(2)
                    - 0.15 * (held - target).powi(2);
                (old.summary.as_ref().is_some_and(|s| s.length_bars >= 4)
                    && next.summary.as_ref().is_none_or(|s| s.length_bars < 4))
                    || next.summary.as_ref().map_or(0., |s| s.score)
                        < old.summary.as_ref().map_or(0., |s| s.score) - 0.05
                    || actual_benefit <= 0.1
                    || merit.min(actual_benefit) < 0.005 * (next.t_in_a - old.t_in_a).max(0.) / rate
            })
        {
            return;
        }
    }
    append(plan, b, target, window, planner.options.stem_playback);
}

fn departure(
    planner: &Planner,
    b: &TrackAnalysis,
    c: &TrackAnalysis,
    plan: &MixPlan,
    pitch: f32,
    exit_cues: &[Cue],
    entry_cues: &[Cue],
) -> Option<MixPlan> {
    let mut options = planner.options.clone();
    options.earliest_outgoing_sec = plan.t_end_b + plan.incoming_offset_end.rate;
    options.outgoing_entry_sec = plan.t_in_b;
    let planner = Planner::with_options(options);
    let mut best: Option<MixPlan> = None;
    for out in cue_choices(exit_cues, true) {
        for input in cue_choices(entry_cues, false) {
            let p = planner.plan_next(&PlanContext {
                outgoing: b,
                incoming: c,
                cues_out: &out,
                cues_in: &input,
                offset_a: PerformanceOffset {
                    pitch_semitones: pitch,
                    ..plan.incoming_offset_end
                },
                offset_b: PerformanceOffset::identity(),
            });
            if p.failure_reason.is_none()
                && best.as_ref().is_none_or(|old| {
                    p.summary.as_ref().map_or(0., |s| s.score)
                        > old.summary.as_ref().map_or(0., |s| s.score)
                })
            {
                best = Some(p);
            }
        }
    }
    best
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (10. + t * (-15. + 6. * t))
}

fn append(plan: &mut MixPlan, b: &TrackAnalysis, target: f32, w: window::Window, stems: bool) {
    let (end_bar, wall) = *plan.clock.nodes.last().unwrap();
    let rate = plan.incoming_offset_end.rate;
    let held = plan.incoming_offset_end.pitch_semitones;
    let depth = (held - target).abs();
    let g = Grid(b);
    let bars_per_second = g.bpm(g.beat(w.start)) * rate / (60. * g.meter());
    let at = |source| end_bar + (source - plan.t_end_b) / rate * bars_per_second;
    let start = at(w.start);
    let end = at(w.end);
    // Pin the old endpoints before extending any envelope: otherwise its last
    // node would interpolate backwards through the already-approved overlap.
    let extend = |line: &mut Polyline, points: Vec<(f32, f32)>| {
        let value = line.sample(end_bar);
        line.nodes.retain(|(u, _)| *u < end_bar);
        line.nodes.push((end_bar, value));
        line.nodes.extend(points);
    };
    let shaped = |neutral: f32, peak: f32| {
        (0..=64)
            .map(|i| {
                let u = i as f32 / 64.;
                let mask = if u < 0.2 {
                    ease(u / 0.2)
                } else if u > 0.8 {
                    ease((1. - u) / 0.2)
                } else {
                    1.
                };
                (start + (end - start) * u, neutral + (peak - neutral) * mask)
            })
            .collect::<Vec<_>>()
    };
    extend(
        &mut plan.lanes.pitch_b,
        (0..=64)
            .map(|i| {
                let u = i as f32 / 64.;
                (
                    start + (end - start) * u,
                    held + (target - held) * ease((u - 0.2) / 0.6),
                )
            })
            .collect(),
    );
    // Hz lanes use log interpolation in the engine; sample the same eased
    // log-frequency curve used for EQ and stem masking.
    let lp = (4200. / (1. + 0.5 * depth)).max(1200.);
    let filter = shaped(20000_f32.ln(), lp.ln())
        .into_iter()
        .map(|(u, v)| (u, v.exp()))
        .collect();
    extend(&mut plan.lanes.filter_b.lp_hz, filter);
    extend(
        &mut plan.lanes.filter_b.hp_hz,
        vec![(start, 20.), (end, 20.)],
    );
    for (line, cut) in [
        (&mut plan.lanes.eq_b.low, depth.min(3.)),
        (&mut plan.lanes.eq_b.mid, (0.5 * depth).min(2.)),
        (&mut plan.lanes.eq_b.high, (0.5 * depth).min(2.)),
    ] {
        extend(line, shaped(0., -cut));
    }
    if stems {
        let mix = plan
            .stem_mix
            .get_or_insert_with(|| mixless_protocol::StemMix {
                outgoing: std::array::from_fn(|_| Polyline::constant(1.)),
                incoming: std::array::from_fn(|_| Polyline::constant(1.)),
            });
        for i in [0, 2, 3] {
            extend(
                &mut mix.incoming[i],
                shaped(1., 10_f32.powf(-depth.min(3.) / 20.)),
            );
        }
        plan.requires_stems = true;
    }
    plan.clock.nodes.extend([
        (start, wall + (w.start - plan.t_end_b) / rate),
        (end, wall + (w.end - plan.t_end_b) / rate),
    ]);
    if let Some(&(u, source)) = plan.incoming_source.nodes.last() {
        let source_end = source + (wall - plan.clock.sample(u)) * rate;
        plan.incoming_source.nodes.retain(|(u, _)| *u < end_bar);
        plan.incoming_source.nodes.extend([
            (end_bar, source_end),
            (start, source_end + w.start - plan.t_end_b),
            (end, source_end + w.end - plan.t_end_b),
        ]);
    }
    plan.stages.push(MixStage {
        start_bar: start,
        end_bar: end,
        label: "Restore pitch".into(),
    });
    plan.t_end_b = w.end;
    plan.incoming_offset_end.pitch_semitones = target;
}
