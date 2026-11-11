//! Shape the two sides of a sequential melodic exchange independently.
//!
//! A sequential stem exchange need not lose amplitude as quickly as a paired
//! crossfade. Retain the tail and reveal the new bed while the opposing tonal
//! parts are closed. Source clocks and the other six parts stay fixed; this
//! does not create new overlap between unrelated keys.
use crate::PlanContext;
use mixless_protocol::{MixPlan, TransitionMode};

fn parts(p: &MixPlan, u: f32) -> [[f32; 4]; 2] {
    let mix = p.stem_mix.as_ref().unwrap();
    let x = ((p.lanes.xfader.sample(u) + 1.) * 0.5).clamp(0., 1.);
    std::array::from_fn(|d| {
        let (level, stems) = if d == 0 {
            (p.lanes.gain_a.sample(u), &mix.outgoing)
        } else {
            (p.lanes.gain_b.sample(u), &mix.incoming)
        };
        let channel = (if d == 0 { 1. - x } else { x }).sqrt()
            * if level <= -90. {
                0.
            } else {
                10f32.powf(level / 20.)
            };
        std::array::from_fn(|s| channel * stems[s].sample(u))
    })
}

pub(crate) fn arrange(p: &mut MixPlan, ctx: &PlanContext<'_>) {
    let Some(s) = &p.summary else { return };
    if s.length_bars < 4
        || p.transition_mode != Some(TransitionMode::BeatBlend)
        || !p.requires_stems
        || p.stem_mix.is_none()
        || ctx.outgoing.stems.is_none()
        || ctx.incoming.stems.is_none()
        || crate::constraints::user_range(ctx.cues_out, ctx.outgoing.sample_rate).is_some()
        || crate::constraints::user_range(ctx.cues_in, ctx.incoming.sample_rate).is_some()
        || p.lanes.loop_a.is_some()
        || p.lanes.loop_b.is_some()
        || p.lanes.scratch_a.is_some()
        || p.lanes.scratch_b.is_some()
        || p.insert_a.is_some()
        || p.send_fx.is_some()
    {
        return;
    }
    if let Some(candidate) = refine(p) {
        if crate::foreground::preserved(&candidate, ctx) {
            *p = candidate;
        }
    }
    // Keep the existing release as a candidate. Then search the two melody
    // contours independently, including compensation for shared channel EQ.
    // This runs on the planning worker; playback only receives polylines.
    for deck in 0..2 {
        search(p, ctx, deck);
    }
}

fn refine(p: &MixPlan) -> Option<MixPlan> {
    contour(p, 0, 0.5, 0.)
}

fn search(p: &mut MixPlan, ctx: &PlanContext<'_>, deck: usize) {
    let original = p.clone();
    let Some(mut best) = cost(&original, ctx, deck) else {
        return;
    };
    for exponent in [1., 0.75, 0.5] {
        // The outgoing side already received its power release above. Do not
        // keep compressing that tail until it becomes an abrupt stop.
        if deck == 0 && exponent != 1. {
            continue;
        }
        for compensation in [0., 0.5, 1.] {
            let Some(candidate) = contour(&original, deck, exponent, compensation) else {
                continue;
            };
            let Some(value) = cost(&candidate, ctx, deck) else {
                continue;
            };
            if value + 1e-5 < best && crate::foreground::preserved(&candidate, ctx) {
                best = value;
                *p = candidate;
            }
        }
    }
}

// Minimize the missing melodic bed and control motion together. Natural
// source silence contributes no weight; source gain cancels in the ratio.
// This is a search objective, not a listening score or a safety override.
fn cost(p: &MixPlan, ctx: &PlanContext<'_>, deck: usize) -> Option<f32> {
    let n = p.summary.as_ref()?.length_bars as f32;
    let (track, source, eq) = if deck == 0 {
        (ctx.outgoing, &p.outgoing_source, &p.lanes.eq_a)
    } else {
        (ctx.incoming, &p.incoming_source, &p.lanes.eq_b)
    };
    let reference = parts(p, if deck == 0 { 0. } else { n })[deck][2];
    if reference <= 0.01 {
        return None;
    }
    let mut sum = 0.;
    let mut weight = 0.;
    let mut previous: Option<(f32, [f32; 2])> = None;
    for i in 0..=(n * 32.) as usize {
        let u = i as f32 / 32.;
        if deck == 1 && u < p.incoming_start_bar {
            continue;
        }
        let frame = track
            .stems
            .as_ref()?
            .frame_at(source.sample(u).min(track.duration_sec - 0.001))?;
        let amplitude = parts(p, u)[deck][2] / reference;
        let gain = [eq.mid.sample(u), eq.high.sample(u)].map(|db| amplitude * 10f32.powf(db / 20.));
        let sec = p.clock.sample(u);
        if let Some((last_sec, last)) = previous {
            let dt = (sec - last_sec).max(1e-4);
            for band in 0..2 {
                let power = 10f32.powf(frame.band_db[2][band + 1] / 10.);
                let missing = (1. - gain[band].powi(2)).max(0.);
                let speed = (gain[band] - last[band]) / dt;
                sum += power * dt * (missing + 0.25 * speed.powi(2));
                weight += power * dt;
            }
        }
        previous = Some((sec, gain));
    }
    (weight > 1e-12).then_some(sum / weight)
}

fn contour(p: &MixPlan, deck: usize, exponent: f32, compensation: f32) -> Option<MixPlan> {
    let n = p.summary.as_ref()?.length_bars as f32;
    let mix = p.stem_mix.as_ref()?;
    let eq = if deck == 0 {
        &p.lanes.eq_a
    } else {
        &p.lanes.eq_b
    };
    let reference = parts(p, if deck == 0 { 0. } else { n })[deck][2];
    if reference <= 0.01 {
        return None;
    }
    let mut points: Vec<f32> = (0..=(n * 64.) as usize).map(|i| i as f32 / 64.).collect();
    for line in [&p.lanes.xfader, &p.lanes.gain_a, &p.lanes.gain_b]
        .into_iter()
        .chain(mix.outgoing.iter())
        .chain(mix.incoming.iter())
        .chain([&eq.mid, &eq.high])
    {
        points.extend(line.nodes.iter().map(|p| p.0));
    }
    points.sort_by(f32::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let mut retained_levels = Vec::with_capacity(points.len());
    let mut changed = false;
    let mut previous = if deck == 0 { reference } else { 0. };
    for &u in &points {
        let g = parts(p, u);
        let melody = g[deck][2];
        // A rise is an intentional re-entry, not the monotonic tail this move
        // handles. Never amplify it or overlap an incoming tonal foreground.
        if (deck == 0 && melody > previous + reference * 0.0001)
            || (deck == 1 && melody < previous - reference * 0.0001)
        {
            return None;
        }
        previous = melody;
        // Compensate at most the less attenuated melodic band. The heard
        // melody therefore never exceeds its own unmixed level in either
        // band. Channel EQ itself stays fixed, preserving the other parts.
        let cut = eq.mid.sample(u).max(eq.high.sample(u)).min(0.);
        let retained = reference
            * (melody / reference).clamp(0., 1.).powf(exponent)
            * 10f32.powf(-cut * compensation / 20.);
        if ((deck == 0 && u == 0.) || (deck == 1 && u >= n))
            && (retained - melody).abs() > reference * 1e-6
        {
            return None;
        }
        if retained > melody + reference * 1e-6
            && [0, 2, 3].into_iter().any(|s| g[1 - deck][s] > 1e-6)
        {
            return None;
        }
        changed |= retained > melody + reference * 0.001;
        retained_levels.push(retained);
    }
    if !changed {
        return None;
    }
    let mut candidate = p.clone();
    let output = candidate.stem_mix.as_mut()?;
    let (output, gain, original) = if deck == 0 {
        (
            &mut output.outgoing,
            &mut candidate.lanes.gain_a,
            &mix.outgoing,
        )
    } else {
        (
            &mut output.incoming,
            &mut candidate.lanes.gain_b,
            &mix.incoming,
        )
    };
    for line in output.iter_mut() {
        line.nodes.clear();
    }
    gain.nodes.clear();
    for (u, retained) in points.into_iter().zip(retained_levels) {
        let old_gain = if deck == 0 {
            &p.lanes.gain_a
        } else {
            &p.lanes.gain_b
        }
        .sample(u);
        let sign = if deck == 0 { -1. } else { 1. };
        let channel = ((1. + sign * p.lanes.xfader.sample(u)) * 0.5)
            .clamp(0., 1.)
            .sqrt()
            * if old_gain <= -90. {
                0.
            } else {
                10f32.powf(old_gain / 20.)
            };
        // Keep the opposite deck and crossfader bit-for-bit intact. Raise
        // only this channel within its existing headroom and compensate its
        // other three stems. The next plan inherits the resulting real entry.
        if old_gain > 6. {
            return None;
        }
        let scale = if channel > 1e-6 {
            (retained / channel)
                .max(1.)
                .min(10f32.powf((6. - old_gain) / 20.))
        } else {
            1.
        };
        gain.nodes.push((u, old_gain + 20. * scale.log10()));
        for s in 0..4 {
            let gain = if s == 2 && channel > 1e-6 {
                (retained / (channel * scale)).clamp(0., 1.)
            } else {
                original[s].sample(u) / scale
            };
            output[s].nodes.push((u, gain));
        }
    }
    Some(candidate)
}

#[cfg(test)]
mod tests;
