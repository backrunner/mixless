//! Optional source envelopes. Existing whole-track EQ remains the safe fallback
//! when the host has not yet loaded both aligned stem buffers.
use super::*;
use mixless_protocol::{Polyline, StemKind, StemMix};

pub(super) fn arrange(ctx: &PlanContext<'_>, plan: &mut MixPlan) {
    // A stem-layered candidate already carries deliberate envelopes.
    if plan.stem_mix.is_some() {
        return;
    }
    let Some(summary) = &plan.summary else {
        return;
    };
    let n = summary.length_bars as f32;
    let duration = plan.clock.sample(n);
    let (Some(a), Some(b)) = (&ctx.outgoing.stems, &ctx.incoming.stems) else {
        return;
    };
    if n <= 0. || duration <= 0.3 {
        return;
    }
    let source = |out: bool, u: f32| {
        let (curve, start, op) = if out {
            (&plan.outgoing_source, plan.t_in_a, &plan.lanes.loop_a)
        } else {
            (&plan.incoming_source, plan.t_in_b, &plan.lanes.loop_b)
        };
        let sec = if curve.nodes.is_empty() {
            start + plan.clock.sample(u)
        } else {
            curve.sample(u)
        };
        if let Some(op) = op.as_ref().filter(|op| u >= op.on_bar && u < op.off_bar) {
            let sr = if out {
                ctx.outgoing.sample_rate
            } else {
                ctx.incoming.sample_rate
            } as f32;
            let start = op.start_src_frame as f32 / sr;
            let length = op.length_src_frames as f32 / sr;
            start + (sec - start).rem_euclid(length.max(0.001))
        } else {
            sec
        }
    };
    // Follow the actual gain crossover, not a fixed number of bars after a drop.
    let center = (0..=128)
        .map(|i| n * i as f32 / 128.)
        .min_by(|x, y| {
            let balance =
                |u: f32| (plan.lanes.gain_a.sample(u) - plan.lanes.gain_b.sample(u)).abs();
            balance(*x).total_cmp(&balance(*y))
        })
        .unwrap_or(n * 0.5);
    let mut vocals = 0;
    let mut drums = 0;
    for i in 0..128 {
        let u = n * (i as f32 + 0.5) / 128.;
        if u < plan.incoming_start_bar
            || plan.lanes.gain_a.sample(u) < -24.
            || plan.lanes.gain_b.sample(u) < -24.
        {
            continue;
        }
        let (Some(a), Some(b)) = (a.frame_at(source(true, u)), b.frame_at(source(false, u))) else {
            continue;
        };
        vocals += usize::from(
            a.vocal_activity > 0.35 && b.vocal_activity > 0.35 && a.rms[0].min(b.rms[0]) > 0.0015,
        );
        drums += usize::from(
            a.rms[1].min(b.rms[1]) > 0.005 && a.band_db[1][0].min(b.band_db[1][0]) > -42.,
        );
    }
    let clash = if !plan.outgoing_source.nodes.is_empty() && !plan.incoming_source.nodes.is_empty()
    {
        crate::progression::tension(
            ctx.outgoing,
            ctx.incoming,
            &plan.outgoing_source,
            &plan.incoming_source,
            n,
            plan.lanes.pitch_a.sample(center),
            plan.lanes.pitch_b.sample(center),
        )
    } else {
        0.
    };
    let enabled = [
        vocals > 2,
        drums > 8,
        clash > 0.55,
        drums > 8 || clash > 0.55,
    ];
    if !enabled.iter().any(|v| *v) {
        return;
    }
    let voice_center = (0..=128)
        .map(|i| n * i as f32 / 128.)
        .filter(|u| *u > plan.incoming_start_bar && *u < n && (*u - center).abs() < n * 0.3)
        .filter(|u| crate::continuity::voice_gap(ctx.outgoing, source(true, *u)) == Some(true))
        .min_by(|x, y| (x - center).abs().total_cmp(&(y - center).abs()))
        .unwrap_or(center);
    let last = plan.clock.nodes.last().map_or(n, |p| p.0);
    let mut result = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    for i in 0..=128 {
        let u = n * i as f32 / 128.;
        let time = plan.clock.sample(u);
        for stem in 0..4 {
            let center_sec = plan
                .clock
                .sample(if stem == 0 { voice_center } else { center });
            // Envelope duration scales with the chosen overlap and is bounded
            // in seconds for smooth controls, not a musical entry constraint.
            let width = if stem == StemKind::Bass.index() {
                (plan.clock.sample((center + 0.125).min(n))
                    - plan.clock.sample((center - 0.125).max(0.)))
                .max(0.01)
            } else {
                (duration * 0.25).clamp(0.15, 8.).min(duration)
            };
            let x = ((time - center_sec) / width + 0.5).clamp(0., 1.);
            let x = x * x * (3. - 2. * x);
            let floor = [0.18, 0.35, 0.25, 0.][stem];
            let (ga, gb) = if enabled[stem] {
                (1. - (1. - floor) * x, floor + (1. - floor) * x)
            } else {
                (1., 1.)
            };
            result.outgoing[stem].nodes.push((
                u,
                if i == 0 || u <= plan.incoming_start_bar {
                    1.
                } else {
                    ga
                },
            ));
            result.incoming[stem]
                .nodes
                .push((u, if i == 128 { 1. } else { gb }));
        }
    }
    if last > n {
        for line in result.outgoing.iter_mut().chain(&mut result.incoming) {
            line.nodes.push((last, line.nodes.last().unwrap().1));
        }
    }
    plan.stem_mix = Some(result);
}

/// Drum-layered blend: the incoming deck plays drums alone until the handoff,
/// where its vocal and instruments arrive while the outgoing tonal material
/// releases. Only valid when both decks will have aligned stem PCM.
pub(super) fn layer(ctx: &PlanContext<'_>, plan: &mut MixPlan, handoff: f32) {
    let Some(summary) = &plan.summary else {
        return;
    };
    let n = summary.length_bars as f32;
    let ease = |v: f32| {
        let v = v.clamp(0., 1.);
        v * v * (3. - 2. * v)
    };
    // Center the outgoing vocal release on the nearest measured breath so a
    // word is never faded mid-syllable.
    let lo = (handoff - 2.).max(0.);
    let hi = (handoff + 1.).min(n);
    let mut vocal_center = handoff + 0.5;
    let mut nearest = f32::MAX;
    let steps = ((hi - lo) * 32.).ceil().max(1.) as usize;
    for i in 0..=steps {
        let u = lo + (hi - lo) * i as f32 / steps as f32;
        if crate::continuity::voice_gap(ctx.outgoing, plan.outgoing_source.sample(u)) == Some(true)
            && (u - handoff).abs() < nearest
        {
            nearest = (u - handoff).abs();
            vocal_center = u;
        }
    }
    let mut result = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    let gain = |u: f32, stem: usize, incoming: bool| {
        let center = if !incoming && stem == 0 {
            vocal_center
        } else {
            handoff + 0.5
        };
        if stem == StemKind::Bass.index() {
            let x = ease((u - handoff) / 0.25);
            if incoming {
                x
            } else {
                1. - x
            }
        } else if stem == 1 {
            1.
        } else if incoming {
            ease((u - (handoff - 0.5)) / 2.)
        } else {
            1. - ease((u - (center - 1.)) / 2.)
        }
    };
    for i in 0..=128 {
        let u = n * i as f32 / 128.;
        for stem in 0..4 {
            result.incoming[stem].nodes.push((u, gain(u, stem, true)));
            result.outgoing[stem].nodes.push((u, gain(u, stem, false)));
        }
    }
    if let Some(last) = plan
        .clock
        .nodes
        .last()
        .map(|node| node.0)
        .filter(|last| *last > n)
    {
        for stem in 0..4 {
            result.incoming[stem]
                .nodes
                .push((last, gain(last, stem, true)));
            result.outgoing[stem]
                .nodes
                .push((last, gain(last, stem, false)));
        }
    }
    plan.requires_stems = true;
    plan.stem_mix = Some(result);
}

/// LoopOut strip-down: the looped outgoing phrase sheds its vocal first, then
/// its instruments through the exchange, while only the incoming drums play
/// until the new foreground arrives on the phrase downbeat.
pub(super) fn loop_out(plan: &mut MixPlan) {
    let Some(summary) = &plan.summary else {
        return;
    };
    let n = summary.length_bars as f32;
    let line = |nodes: &[(f32, f32)]| Polyline {
        nodes: nodes.to_vec(),
    };
    let mut mix = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::constant(1.)),
        incoming: std::array::from_fn(|_| Polyline::constant(1.)),
    };
    mix.outgoing[StemKind::Vocals.index()] = line(&[(0., 1.), (0.25, 0.), (n, 0.)]);
    mix.outgoing[StemKind::Instruments.index()] =
        line(&[(0., 1.), (1., 1.), (n / 2., 0.), (n, 0.)]);
    mix.incoming[StemKind::Vocals.index()] =
        line(&[(0., 0.), (n / 2., 0.), (n / 2. + 1., 1.), (n, 1.)]);
    mix.incoming[StemKind::Instruments.index()] = mix.incoming[StemKind::Vocals.index()].clone();
    mix.outgoing[StemKind::Bass.index()] =
        line(&[(0., 1.), (n / 2., 1.), (n / 2. + 0.25, 0.), (n, 0.)]);
    mix.incoming[StemKind::Bass.index()] =
        line(&[(0., 0.), (n / 2., 0.), (n / 2. + 0.25, 1.), (n, 1.)]);
    plan.requires_stems = true;
    plan.stem_mix = Some(mix);
}
