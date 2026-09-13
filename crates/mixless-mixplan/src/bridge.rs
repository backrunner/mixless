//! Actual overlap for filter, echo and dry phrase transitions. A transition
//! name must not disguise a last-beat launch and a 40 ms crossfader cut.
use crate::{
    constraints::covers_user_range,
    grid::Grid,
    musical::{feature, percussion_only},
    policy::Technique,
    smooth::grid_reliable,
    PlanContext,
};
use mixless_protocol::{BarMap, MixPlan, MixStage, Polyline, StrategyId};

fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (10. + t * (-15. + 6. * t))
}
fn db(gain: f32) -> f32 {
    if gain <= 0.00002 {
        -96.
    } else {
        (20. * gain.log10()).clamp(-96., 6.)
    }
}

pub(super) fn expand(
    plan: &mut MixPlan,
    ctx: &PlanContext<'_>,
    technique: Technique,
    harmonic: bool,
) -> Option<()> {
    let (a, b) = (ctx.outgoing, ctx.incoming);
    let (ga, gb) = (Grid(a), Grid(b));
    let n = plan.summary.as_ref()?.length_bars as f32;
    let seconds = plan.clock.sample(n);
    if seconds < 0.5 {
        return None;
    }
    let a_start = ga.beat(plan.t_in_a);
    let b_start = gb.beat(plan.t_in_b);
    let ratio = ga.bpm(a_start) * ctx.offset_a.rate / (gb.bpm(b_start) * ctx.offset_b.rate);
    let factor = if (ratio / 2. - 1.).abs() < 0.08 {
        0.5
    } else if (ratio / 0.5 - 1.).abs() < 0.08 {
        2.
    } else {
        1.
    };
    let locked = (ratio * factor - 1.).abs() <= 0.08
        && ga.meter() == gb.meter()
        && grid_reliable(a, plan.t_in_a, plan.t_out_a)
        && grid_reliable(
            b,
            plan.t_in_b,
            gb.sec(b_start + n * ga.meter() * factor)
                .min(b.duration_sec),
        );
    // Without trustworthy beat alignment use a restrained phrase overlap;
    // never let two unsynchronized drum patterns run together for half a minute.
    if !locked && n > 1. {
        return None;
    }
    let b_end = if locked {
        gb.sec(b_start + n * ga.meter() * factor)
    } else {
        plan.t_in_b + seconds * ctx.offset_b.rate
    };
    if b_end > b.duration_sec - 0.1
        || !covers_user_range(plan.t_in_b, b_end, ctx.cues_in, b.sample_rate)
    {
        return None;
    }
    if b.mix_regions.iter().any(|r| {
        r.kind == mixless_protocol::MixRegionKind::In && (r.anchor_sec - plan.t_in_b).abs() < 0.15
    }) && !b.mix_regions.iter().any(|r| {
        r.kind == mixless_protocol::MixRegionKind::In
            && (r.anchor_sec - plan.t_in_b).abs() < 0.15
            && b_end <= r.end_sec + 0.05
    }) {
        return None;
    }
    if !harmonic
        && n > 2.
        && !percussion_only(a, plan.t_in_a, plan.t_out_a)
        && !percussion_only(b, plan.t_in_b, b_end)
    {
        return None;
    }
    plan.bar_map = if locked && factor != 1. {
        BarMap::TwoToOne {
            outgoing_is_double: factor < 1.,
        }
    } else {
        BarMap::OneToOne
    };
    let mut voice = [0f32; 2];
    for i in 0..64 {
        let u = (i as f32 + 0.5) / 64. * n;
        let ta = plan.outgoing_source.sample(u);
        let tb = if locked {
            gb.sec(b_start + u * ga.meter() * factor)
        } else {
            plan.t_in_b + plan.clock.sample(u) * ctx.offset_b.rate
        };
        let va = feature(a, ta).map_or(1., |f| {
            if harmonic {
                crate::vocals::risk(f)
            } else {
                f.vocal_presence
            }
        });
        let vb = feature(b, tb).map_or(1., |f| {
            if harmonic {
                crate::vocals::risk(f)
            } else {
                f.vocal_presence
            }
        });
        voice[0] = voice[0].max(va);
        voice[1] = voice[1].max(vb);
        if va > 0.5 && vb > 0.5 {
            return None;
        }
    }
    let lanes = &mut plan.lanes;
    let trim = lanes.gain_b.sample(0.);
    for lane in [
        &mut lanes.xfader,
        &mut lanes.gain_a,
        &mut lanes.gain_b,
        &mut lanes.eq_a.low,
        &mut lanes.eq_b.low,
        &mut lanes.eq_a.mid,
        &mut lanes.eq_b.mid,
        &mut lanes.eq_a.high,
        &mut lanes.eq_b.high,
        &mut lanes.filter_a.lp_hz,
        &mut lanes.filter_b.hp_hz,
        &mut lanes.fx_send_a,
        &mut lanes.rate_b,
    ] {
        lane.nodes.clear();
    }
    plan.clock.nodes.retain(|(u, _)| *u <= n);
    plan.outgoing_source.nodes.retain(|(u, _)| *u <= n);
    plan.incoming_source = Polyline::default();
    plan.incoming_start_bar = 0.;
    plan.t_end_b = b_end;
    plan.handoff_bar = Some(n * 0.55);
    let echo = technique == Technique::EchoOut;
    let filter = technique == Technique::FilterBridge;
    // Different foregrounds enter in stages; no filter reset while audible.
    for i in 0..=n as usize * 64 {
        let u = i as f32 / 64.;
        let t = u / n;
        let p = ease((t - 0.08) / 0.84);
        let xf = 2. * p - 1.;
        let angle = p * std::f32::consts::FRAC_PI_2;
        let la = 1. - ease((t - 0.88) / 0.12);
        let lb = ease(t / 0.12);
        lanes.xfader.nodes.push((u, xf));
        lanes.gain_a.nodes.push((u, db(la)));
        lanes.gain_b.nodes.push((u, (trim + db(lb)).max(-96.)));
        let bass_width = if locked {
            (0.5 / (ga.meter() * n)).max(0.02)
        } else {
            0.16
        };
        let bass =
            ease((t - (0.55 - bass_width)) / (2. * bass_width)) * std::f32::consts::FRAC_PI_2;
        lanes
            .eq_a
            .low
            .nodes
            .push((u, db(bass.cos().max(0.) / (angle.cos() * la).max(0.00002))));
        lanes
            .eq_b
            .low
            .nodes
            .push((u, db(bass.sin().max(0.) / (angle.sin() * lb).max(0.00002))));
        lanes.eq_a.mid.nodes.push((
            u,
            -if voice[1] > 0.5 { 12. } else { 4. } * ease((t - 0.4) / 0.5),
        ));
        lanes.eq_b.mid.nodes.push((
            u,
            -if voice[0] > 0.5 { 12. } else { 4. } * (1. - ease((t - 0.2) / 0.65)),
        ));
        lanes
            .eq_a
            .high
            .nodes
            .push((u, -6. * ease((t - 0.25) / 0.7)));
        lanes.eq_b.high.nodes.push((u, -3. * (1. - ease(t / 0.4))));
        lanes.filter_a.lp_hz.nodes.push((
            u,
            if filter {
                (20000f32.ln() + ease((t - 0.30) / 0.65) * (1600f32 / 20000.).ln()).exp()
            } else {
                20000.
            },
        ));
        lanes.filter_b.hp_hz.nodes.push((
            u,
            if filter {
                (450f32.ln() + ease(t / 0.7) * (20f32 / 450.).ln()).exp()
            } else {
                20.
            },
        ));
        lanes.fx_send_a.nodes.push((
            u,
            if echo {
                0.20 * ease((t - 0.68) / 0.16) * (1. - ease((t - 0.90) / 0.10))
            } else {
                0.
            },
        ));
        let beat = b_start + u * ga.meter() * factor;
        let rate = if locked {
            ga.bpm(a_start + u * ga.meter()) * ctx.offset_a.rate * factor / gb.bpm(beat)
        } else {
            ctx.offset_b.rate
        };
        if !(0.5..=2.).contains(&rate) || !(0.88..=1.12).contains(&(rate / ctx.offset_b.rate)) {
            return None;
        }
        lanes.rate_b.nodes.push((u, rate));
        plan.incoming_source.nodes.push((
            u,
            if locked {
                gb.sec(beat)
            } else {
                plan.t_in_b + plan.clock.sample(u) * ctx.offset_b.rate
            },
        ));
    }
    plan.incoming_offset_end.rate = lanes.rate_b.sample(n);
    plan.summary.as_mut()?.strategy = if echo {
        StrategyId::EchoOut
    } else if filter {
        StrategyId::FilterSweep
    } else {
        StrategyId::PhraseBlend
    };
    plan.stages = vec![
        MixStage {
            start_bar: 0.,
            end_bar: n * 0.30,
            label: if filter {
                "Introduce filtered rhythm"
            } else {
                "Introduce rhythm"
            }
            .into(),
        },
        MixStage {
            start_bar: n * 0.30,
            end_bar: n * 0.55,
            label: "Open blend · exchange highs".into(),
        },
        MixStage {
            start_bar: n * 0.55,
            end_bar: n * 0.88,
            label: if echo {
                "Bass exchange · echo tail".into()
            } else if filter {
                "Bass exchange · filter release".into()
            } else {
                "Bass exchange".into()
            },
        },
        MixStage {
            start_bar: n * 0.88,
            end_bar: n,
            label: "Close outgoing level".into(),
        },
    ];
    Some(())
}
