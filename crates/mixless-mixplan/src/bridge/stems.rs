//! Native-tempo handover, fitted to the heard parts rather than an FX preset.
//! Search and band/level prediction happen on the host, never in the callback.
use crate::{PlanContext, grid::Grid};
use mixless_protocol::{MixPlan, MixStage, Polyline, StemMix, StrategyId, TrackAnalysis};

#[derive(Clone, Copy, Debug)]
struct Fade {
    start: f32,
    end: f32,
}

fn ease(x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    x * x * x * (10. + x * (-15. + 6. * x))
}
impl Fade {
    fn at(self, sec: f32) -> f32 {
        ease((sec - self.start) / (self.end - self.start).max(0.001))
    }
}
fn db(x: f32) -> f32 {
    (20. * x.max(0.000016).log10()).clamp(-96., 6.)
}

#[derive(Clone, Copy)]
struct Frame {
    sec: f32,
    u: f32,
    // Analysis meters merge bass and other instruments. Use the measured low
    // band as a bass proxy, never pretend these are four independent meters.
    bands: [[[f32; 3]; 4]; 2],
    voice: [f32; 2],
    onset: [[f32; 3]; 2],
}
type Shape = [[Fade; 4]; 2];

fn powers(t: &TrackAnalysis, sec: f32, width: f32) -> Option<([[f32; 3]; 4], f32, [f32; 3])> {
    let meters = t.stems.as_ref()?;
    let mut bands = [[0.; 3]; 4];
    let mut voice = 0f32;
    let mut onset = [0f32; 3];
    for i in 0..5 {
        let s = (sec + (i as f32 / 4. - 0.5) * width).clamp(0., t.duration_sec - 0.001);
        let f = meters.frame_at(s)?;
        voice = voice.max(f.vocal_activity);
        for part in 0..3 {
            onset[part] = onset[part].max(f.onset[part]);
            for band in 0..3 {
                let target = if part == 2 && band == 0 { 3 } else { part };
                bands[target][band] += 10f32.powf(f.band_db[part][band] / 10.) / 5.;
            }
        }
    }
    Some((bands, voice, onset))
}

fn gains(shape: &Shape, sec: f32) -> [[f32; 4]; 2] {
    std::array::from_fn(|deck| {
        std::array::from_fn(|part| {
            let x = shape[deck][part].at(sec);
            if deck == 0 { 1. - x } else { x }
        })
    })
}
fn band_power(f: &Frame, g: [[f32; 4]; 2]) -> [[f32; 3]; 2] {
    std::array::from_fn(|d| {
        std::array::from_fn(|band| (0..4).map(|s| f.bands[d][s][band] * g[d][s].powi(2)).sum())
    })
}
fn full(f: &Frame, deck: usize) -> f32 {
    f.bands[deck].iter().flatten().sum::<f32>()
}

/// A cost in relative power/dB: gain scaling cannot change the selected moves.
/// Missing energy and two unrelated rhythmic foregrounds cost more than a
/// gentle overlap. An absent pad cannot be invented to cover a drum exchange.
fn cost(frames: &[Frame], shape: &Shape, harmonic: bool) -> Option<f32> {
    let duration = frames.last()?.sec;
    let mut loss = 0.;
    for f in frames {
        let g = gains(shape, f.sec);
        if g[0][1] * g[1][1] > 0.0001 || g[0][3] * g[1][3] > 0.0001 {
            return None;
        }
        if !harmonic {
            let a = g[0][0].max(g[0][2]).max(g[0][3]);
            let b = g[1][0].max(g[1][2]).max(g[1][3]);
            if a * b > 0.0001 {
                return None;
            }
        }
        if f.voice[0].min(f.voice[1]) > 0.35 && g[0][0] * g[1][0] > 0.01 {
            return None;
        }
        let p = band_power(f, g);
        let power = p.iter().flatten().sum::<f32>();
        // A plucked chord, bass pulse or vocal chop also carries rhythm.
        // Muting only the drum lane does not make its deck beatless. Native
        // clocks may overlap a sustained bed, never two audible attack beds.
        let rhythmic: [f32; 2] = std::array::from_fn(|d| {
            (0..4)
                .filter(|s| *s == 1 || f.onset[d][(*s).min(2)] >= 0.4)
                .map(|s| f.bands[d][s].iter().sum::<f32>() * g[d][s].powi(2))
                .sum()
        });
        if rhythmic[0].min(rhythmic[1]) > power.max(1e-12) * 0.01 {
            return None;
        }
        let x = ease(f.sec / duration);
        let target = (full(f, 0) * (1. - x) + full(f, 1) * x).max(1e-12);
        let delta = 10. * (power.max(1e-12) / target).log10();
        loss += delta.min(0.).powi(2) + 0.25 * delta.max(0.).powi(2);
        // Tonal stems may also contain plucked/arpeggiated rhythm. Prefer
        // windows where those attacks do not compete across the tempo gap.
        loss += 6.
            * (0..3)
                .map(|s| f.onset[0][s].min(f.onset[1][s]) * g[0][s] * g[1][s])
                .sum::<f32>();
    }
    Some(loss / frames.len() as f32)
}

fn voice_safe(ctx: &PlanContext<'_>, plan: &MixPlan, shape: &Shape) -> bool {
    let source = |deck: usize, sec: f32| {
        if deck == 0 {
            plan.t_in_a + sec * ctx.offset_a.rate
        } else {
            plan.t_in_b + sec * ctx.offset_b.rate
        }
    };
    crate::vocals::safe_exit(ctx.outgoing, source(0, shape[0][0].end))
        && crate::vocals::safe_entry(ctx.incoming, source(1, shape[1][0].start))
}

fn fit(plan: &MixPlan, ctx: &PlanContext<'_>, frames: &[Frame], harmonic: bool) -> Option<Shape> {
    let duration = frames.last()?.sec;
    let (ga, gb) = (Grid(ctx.outgoing), Grid(ctx.incoming));
    let beat = [
        60. / (ga.bpm(ga.beat(plan.t_in_a)) * ctx.offset_a.rate),
        60. / (gb.bpm(gb.beat(plan.t_in_b)) * ctx.offset_b.rate),
    ];
    // Rhythm can restart only on B's own downbeat. Both decks keep native
    // rates; mapping their unrelated clocks to the same bar would be false.
    let mut boundaries: Vec<f32> = ctx
        .incoming
        .tempo
        .downbeats
        .iter()
        .map(|s| (*s - plan.t_in_b) / ctx.offset_b.rate)
        .filter(|s| *s >= 2. * beat[0] && *s <= duration - 2. * beat[1])
        .collect();
    if boundaries.len() > 12 {
        boundaries = (0..12)
            .map(|i| boundaries[i * (boundaries.len() - 1) / 11])
            .collect();
    }
    let mut best = None;
    let mut best_cost = f32::INFINITY;
    for rhythm in boundaries {
        if crate::constraints::user_range(ctx.cues_out, ctx.outgoing.sample_rate).is_none()
            && crate::drops::inside_peak(ctx.outgoing, plan.t_in_a + rhythm * ctx.offset_a.rate)
        {
            continue;
        }
        // The tonal exchange is independent of the kit. It may happen before
        // or after it, while the other parts carry the measured energy.
        for gate in frames
            .iter()
            .step_by((frames.len() / 8).max(1))
            .map(|f| f.sec)
            .filter(|s| *s > 2. * beat[0] && *s < duration - 2. * beat[1])
        {
            let mut shape = [[Fade {
                start: 0.,
                end: duration,
            }; 4]; 2];
            for part in 0..4 {
                let centre = if part == 1 || part == 3 { rhythm } else { gate };
                let overlap = harmonic && part == 2;
                shape[0][part] = Fade {
                    start: (centre - 2. * beat[0]).max(0.),
                    end: if overlap {
                        (centre + beat[0]).min(duration)
                    } else if !harmonic && part != 1 {
                        centre.min(gate)
                    } else {
                        centre
                    },
                };
                shape[1][part] = Fade {
                    start: if overlap {
                        (centre - beat[1]).max(0.)
                    } else if !harmonic && part != 1 {
                        centre.max(gate)
                    } else {
                        centre
                    },
                    end: (centre.max(if !harmonic && part != 1 { gate } else { 0. })
                        + 2. * beat[1])
                        .min(duration),
                };
                if !overlap {
                    shape[0][part].start = (shape[0][part].end - 2. * beat[0]).max(0.);
                }
            }
            // A 1e-4 cost tie tolerance keeps roundoff from changing the
            // chosen curves when a recording is rescaled.
            // Coordinate descent gives each part/deck its own fade width.
            // Evaluate the complete sum, including the energy left by the
            // other parts, instead of four independently attractive holes.
            for _ in 0..2 {
                for part in [2, 0, 1, 3] {
                    for deck in 0..2 {
                        let mut chosen = shape;
                        let mut score = cost(frames, &shape, harmonic).unwrap_or(f32::INFINITY);
                        for beats in [2., 4., 8.] {
                            let mut candidate = shape;
                            let fade = &mut candidate[deck][part];
                            if deck == 0 {
                                fade.start = (fade.end - beats * beat[0]).max(0.);
                            } else {
                                fade.end = (fade.start + beats * beat[1]).min(duration);
                            }
                            if let Some(c) =
                                cost(frames, &candidate, harmonic).filter(|c| *c < score - 0.0001)
                            {
                                score = c;
                                chosen = candidate;
                            }
                        }
                        shape = chosen;
                    }
                }
            }
            if !voice_safe(ctx, plan, &shape) {
                continue;
            }
            // An incoming peak must arrive complete; do not spend its first
            // phrase hidden behind the outgoing song's part envelopes.
            if crate::constraints::user_range(ctx.cues_in, ctx.incoming.sample_rate).is_none()
                && crate::drops::peaks(ctx.incoming).iter().any(|p| {
                    p.0 >= plan.t_in_b - 0.05
                        && p.0 < plan.t_end_b - 0.05
                        && shape[1]
                            .iter()
                            .any(|f| plan.t_in_b + f.end * ctx.offset_b.rate > p.0 + 0.05)
                })
            {
                continue;
            }
            if let Some(c) = cost(frames, &shape, harmonic).filter(|c| *c < best_cost - 0.0001) {
                best_cost = c;
                best = Some(shape);
            }
        }
    }
    best
}

fn at_bar(clock: &Polyline, sec: f32) -> f32 {
    let right = clock
        .nodes
        .partition_point(|p| p.1 < sec)
        .min(clock.nodes.len() - 1);
    if right == 0 {
        return clock.nodes[0].0;
    }
    let (a, b) = (clock.nodes[right - 1], clock.nodes[right]);
    a.0 + (b.0 - a.0) * ((sec - a.1) / (b.1 - a.1)).clamp(0., 1.)
}

pub(super) fn arrange(plan: &mut MixPlan, ctx: &PlanContext<'_>, harmonic: bool) -> Option<()> {
    let n = plan.summary.as_ref()?.length_bars as f32;
    let duration = plan.clock.sample(n);
    let count = (duration / 0.1).ceil().clamp(64., 256.) as usize;
    let mut frames = Vec::with_capacity(count + 1);
    for i in 0..=count {
        let u = n * i as f32 / count as f32;
        let sec = plan.clock.sample(u);
        let a = powers(
            ctx.outgoing,
            plan.outgoing_source.sample(u),
            60. / ctx.outgoing.tempo.global_bpm,
        )?;
        let b = powers(
            ctx.incoming,
            plan.incoming_source.sample(u),
            60. / ctx.incoming.tempo.global_bpm,
        )?;
        frames.push(Frame {
            u,
            sec,
            bands: [a.0, b.0],
            voice: [a.1, b.1],
            onset: [a.2, b.2],
        });
    }
    let shape = fit(plan, ctx, &frames, harmonic).or_else(|| miss!())?;
    // Keep the exact fade boundaries. Sampling on a regular grid alone can
    // interpolate a sliver of both kits (or both keys) across their handoff.
    let mut times: Vec<f32> = frames.iter().map(|f| f.sec).collect();
    times.extend(shape.iter().flatten().flat_map(|f| [f.start, f.end]));
    times.sort_by(f32::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    frames.clear();
    for sec in times {
        let u = at_bar(&plan.clock, sec);
        let a = powers(
            ctx.outgoing,
            plan.outgoing_source.sample(u),
            60. / ctx.outgoing.tempo.global_bpm,
        )?;
        let b = powers(
            ctx.incoming,
            plan.incoming_source.sample(u),
            60. / ctx.incoming.tempo.global_bpm,
        )?;
        frames.push(Frame {
            u,
            sec,
            bands: [a.0, b.0],
            voice: [a.1, b.1],
            onset: [a.2, b.2],
        });
    }
    let last = frames.len() - 1;
    let mut mix = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    let trim = plan.lanes.gain_b.sample(0.);
    let lanes = &mut plan.lanes;
    for line in [
        &mut lanes.xfader,
        &mut lanes.gain_a,
        &mut lanes.gain_b,
        &mut lanes.eq_a.low,
        &mut lanes.eq_a.mid,
        &mut lanes.eq_a.high,
        &mut lanes.eq_b.low,
        &mut lanes.eq_b.mid,
        &mut lanes.eq_b.high,
    ] {
        line.nodes.clear();
    }
    for filter in [&mut lanes.filter_a, &mut lanes.filter_b] {
        filter.lp_hz = Polyline::constant(20000.);
        filter.hp_hz = Polyline::constant(20.);
    }
    lanes.fx_send_a = Polyline::constant(0.);
    lanes.fx_send_b = Polyline::constant(0.);
    plan.send_fx = None;
    // Open the crossfader while compensating its exact square-root law.
    // Independent part envelopes and channel levels perform the actual mix.
    let opening = shape[1]
        .iter()
        .map(|f| f.start)
        .fold(duration, f32::min)
        .max(0.05);
    let closing = shape[0]
        .iter()
        .map(|f| f.end)
        .fold(0., f32::max)
        .min(duration - 0.05);
    let mut previous_eq = [[0f32; 3]; 2];
    let mut previous_gain = [0f32; 2];
    for (i, f) in frames.iter().enumerate() {
        let g = gains(&shape, f.sec);
        for s in 0..4 {
            mix.outgoing[s].nodes.push((f.u, g[0][s]));
            mix.incoming[s].nodes.push((f.u, g[1][s]));
        }
        let x = if f.sec < opening {
            -1. + ease(f.sec / opening)
        } else if f.sec > closing {
            ease((f.sec - closing) / (duration - closing))
        } else {
            0.
        };
        lanes.xfader.nodes.push((f.u, x));
        let p = band_power(f, g);
        let progress = ease(f.sec / duration);
        let dt = if i == 0 {
            0.
        } else {
            f.sec - frames[i - 1].sec
        };
        let mut eq = [[0f32; 3]; 2];
        for band in 0..3 {
            let total = p[0][band] + p[1][band];
            let target = (0..4)
                .map(|s| f.bands[0][s][band] * (1. - progress) + f.bands[1][s][band] * progress)
                .sum::<f32>();
            let excess = (10. * (total.max(1e-12) / target.max(1e-12)).log10()).clamp(0., 6.);
            for d in 0..2 {
                // Remove shared-band masking from the deck contributing it.
                // Never boost a missing band or change tone at either endpoint.
                let shade = (std::f32::consts::PI * progress).sin().powi(2);
                let cut = -excess * p[d][band] / total.max(1e-12) * shade;
                eq[d][band] = cut
                    .clamp(previous_eq[d][band] - dt, previous_eq[d][band] + dt)
                    .max(-(duration - f.sec));
            }
        }
        let heard = std::array::from_fn::<_, 2, _>(|d| {
            (0..3)
                .map(|band| p[d][band] * 10f32.powf(eq[d][band] / 10.))
                .sum::<f32>()
        });
        let target = full(f, 0) * (1. - progress) + full(f, 1) * progress;
        let total = heard.iter().sum::<f32>();
        for d in 0..2 {
            let over = (10. * (total.max(1e-12) / target.max(1e-12)).log10()).clamp(0., 6.);
            let duck = (-over * heard[d] / total.max(1e-12))
                .clamp(previous_gain[d] - dt, previous_gain[d] + dt)
                .max(-(duration - f.sec));
            previous_gain[d] = duck;
            let fader = if d == 0 {
                ((1. - x) / 2.).sqrt()
            } else {
                ((1. + x) / 2.).sqrt()
            };
            let level = -db(fader).min(0.) + duck + if d == 1 { trim } else { 0. };
            let line = if d == 0 {
                &mut lanes.gain_a
            } else {
                &mut lanes.gain_b
            };
            line.nodes.push((
                f.u,
                if d == 0 && i == last {
                    -96.
                } else {
                    level.min(6.)
                },
            ));
            let dest = if d == 0 {
                &mut lanes.eq_a
            } else {
                &mut lanes.eq_b
            };
            for (line, v) in [&mut dest.low, &mut dest.mid, &mut dest.high]
                .into_iter()
                .zip(eq[d])
            {
                line.nodes.push((f.u, v));
            }
        }
        previous_eq = eq;
    }
    plan.requires_stems = true;
    plan.stem_mix = Some(mix);
    let to_bar = |sec: f32| at_bar(&plan.clock, sec);
    let rhythm = shape[1][1].start;
    plan.handoff_bar = Some(to_bar(rhythm));
    plan.summary.as_mut()?.strategy = StrategyId::PhraseBlend;
    plan.stages = vec![
        MixStage {
            start_bar: 0.,
            end_bar: to_bar(shape[0][1].start),
            label: "Introduce incoming parts".into(),
        },
        MixStage {
            start_bar: to_bar(shape[0][1].start),
            end_bar: to_bar(shape[1][1].end),
            label: "Exchange rhythm over the tonal bed".into(),
        },
        MixStage {
            start_bar: to_bar(shape[1][1].end),
            end_bar: n,
            label: "Complete the part and level handover".into(),
        },
    ];
    plan.stages.retain(|s| s.end_bar > s.start_bar);
    // A breakdown can start with a loud impact and naturally settle. Judge
    // added attenuation against the sources at EACH instant, not a straight
    // line from that first transient to the final sample.
    let mut reference = plan.clone();
    reference.stem_mix = None;
    reference.lanes.eq_a = Default::default();
    reference.lanes.eq_b = Default::default();
    reference.lanes.gain_a = Polyline::constant(0.);
    reference.lanes.gain_b = Polyline::constant(trim);
    reference.lanes.xfader.nodes = frames
        .iter()
        .map(|f| (f.u, 2. * ease(f.sec / duration) - 1.))
        .collect();
    if frames.iter().any(|f| {
        crate::level::at(&reference, ctx.outgoing, ctx.incoming, f.u)
            - crate::level::at(plan, ctx.outgoing, ctx.incoming, f.u)
            > 9.
    }) {
        return miss!();
    }

    Some(())
}

#[cfg(test)]
mod tests;
