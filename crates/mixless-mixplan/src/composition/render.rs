use super::{gains, highpass, power, Frame, Shape};
use mixless_protocol::{MixPlan, Polyline, StemMix};

/// Source-clock observations shared by every shape of one arrangement.
pub(super) struct Prepared {
    voices: [Vec<(f32, f32)>; 2],
    points: Vec<f32>,
}

impl Prepared {
    pub(super) fn new(p: &MixPlan, ctx: &crate::PlanContext<'_>, n: f32) -> Self {
        let voices: [Vec<(f32, f32)>; 2] = std::array::from_fn(|d| {
            let (t, source) = if d == 0 {
                (ctx.outgoing, &p.outgoing_source)
            } else {
                (ctx.incoming, &p.incoming_source)
            };
            let start = source.sample(0.);
            let end = source.sample(n);
            let notes: Vec<_> = t
                .stems
                .iter()
                .flat_map(|s| &s.notes)
                .filter(|note| {
                    note.stem == mixless_protocol::StemKind::Vocals
                        && note.confidence >= 0.45
                        && note.end_sec > start
                        && note.start_sec < end
                })
                .collect();
            t.stems
                .iter()
                .flat_map(|s| &s.frames)
                .filter(|f| {
                    f.end_sec > start
                        && f.start_sec <= end
                        && (f.vocal_activity >= 0.2
                            || notes.iter().any(|note| {
                                note.start_sec < f.end_sec && note.end_sec > f.start_sec
                            }))
                        && f.rms[0].powi(2) > (f.rms[1].powi(2) + f.rms[2].powi(2)) * 0.0001
                })
                .map(|f| {
                    let u = super::bar_at(source, f.start_sec);
                    (u, p.clock.sample(u))
                })
                .collect()
        });
        Self {
            voices,
            points: (0..=n as usize * 64).map(|i| i as f32 / 64.).collect(),
        }
    }
}

pub(super) fn apply_prepared(
    p: &mut MixPlan,
    frames: &[Frame],
    shape: &Shape,
    prepared: &Prepared,
) -> Option<()> {
    let n = p.summary.as_ref()?.length_bars as f32;
    // Channel EQ also attenuates the singer. Reserve its restoration before
    // each audible vocal entrance, even while that lane is still muted.
    // Exact source frames avoid losing a word between the fitter's coarse
    // energy samples. No extra analysis field or callback work is needed.
    let voice_times: [Vec<f32>; 2] = std::array::from_fn(|d| {
        prepared.voices[d]
            .iter()
            .filter_map(|&(u, sec)| {
                let gain = shape[d][0].at(u);
                let gain = if d == 0 { 1. - gain } else { gain };
                (gain > 0.01).then_some(sec)
            })
            .collect()
    });
    let last = p.clock.nodes.last()?.0.max(n);
    let mut points = prepared.points.clone();
    points.extend(shape.iter().flatten().flat_map(|f| [f.start, f.end]));
    points.sort_by(f32::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let mut mix = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    let mut eq = [
        [
            Polyline::default(),
            Polyline::default(),
            Polyline::default(),
        ],
        [
            Polyline::default(),
            Polyline::default(),
            Polyline::default(),
        ],
    ];
    let mut previous = [[0f32; 3]; 2];
    let mut prev_sec = 0.;
    let mut previous_x = -1.;
    let l = &mut p.lanes;
    for line in [&mut l.xfader, &mut l.gain_a, &mut l.gain_b] {
        line.nodes.clear();
    }
    // Filter only a genuinely isolated rhythm layer. Its bass and tonal
    // stems remain governed independently; the full incoming track ends dry.
    for filter in [&mut l.filter_a, &mut l.filter_b] {
        filter.hp_hz = Polyline::default();
        filter.lp_hz = Polyline::constant(20000.);
    }
    for u in points {
        let f = &frames[((u / n * 128.).round() as usize).min(128)];
        let sec = p.clock.sample(u);
        let dt = sec - prev_sec;
        let remaining = p.clock.sample(n) - sec;
        let g = gains(shape, u);
        let band = power(f, g);
        l.filter_a.hp_hz.nodes.push((u, highpass(g[0], g[1])));
        l.filter_b.hp_hz.nodes.push((u, highpass(g[1], g[0])));
        let progress = shape[1][2].at(u);
        for b in 0..3 {
            // Keep a single sung foreground; otherwise follow the selected
            // tonal exchange. A quieter background gives up shared bands.
            let owner = if f.voice[0] > 0.35 && g[0][0] > 0.5 {
                0
            } else if f.voice[1] > 0.35 && g[1][0] > 0.5 {
                1
            } else {
                usize::from(progress >= 0.5)
            };
            let background = 1 - owner;
            let target = if b == 0 {
                0.
            } else {
                (10. * (band[owner][b].max(1e-12) * 0.125 / band[background][b].max(1e-12)).log10())
                    .clamp(-12., 0.)
            };
            for d in 0..2 {
                let requested = if d == background { target } else { 0. };
                // One dB per second, with enough reserved time to finish dry.
                let cut = requested
                    .clamp(previous[d][b] - dt, previous[d][b] + dt)
                    .max(-remaining)
                    .max(
                        voice_times[d]
                            .get(voice_times[d].partition_point(|at| *at < sec - 0.05))
                            .map_or(-12., |at| -(at - sec - 0.05).max(0.)),
                    );
                eq[d][b].nodes.push((u, cut));
                previous[d][b] = cut;
            }
        }
        let routing = crate::part_routing::route(g);
        let x = routing.crossfader;
        l.xfader.nodes.push((u, x));
        for d in 0..2 {
            // Route the outgoing envelope through the crossfader first; its
            // channel fader is only housekeeping once that side is closed.
            let gain = if d == 0 && x == 1. && (previous_x == 1. || u >= n) {
                -96.
            } else {
                routing.gain_db
            };
            let lane = if d == 0 { &mut l.gain_a } else { &mut l.gain_b };
            lane.nodes.push((u, gain));
            let stems = if d == 0 {
                &mut mix.outgoing
            } else {
                &mut mix.incoming
            };
            for s in 0..4 {
                let ratio = routing.stems[d][s].max(f32::from(d == 1 && u >= n));
                stems[s].nodes.push((u, ratio));
            }
        }
        prev_sec = sec;
        previous_x = x;
    }
    for (d, lanes) in [&mut l.eq_a, &mut l.eq_b].into_iter().enumerate() {
        [lanes.low, lanes.mid, lanes.high] = eq[d].clone();
    }
    l.fx_send_a = Polyline::constant(0.);
    l.fx_send_b = Polyline::constant(0.);
    p.send_fx = None;
    p.requires_stems = true;
    if last > n {
        for line in &mut mix.outgoing {
            line.nodes.push((last, 0.));
        }
        for line in &mut mix.incoming {
            line.nodes.push((last, 1.));
        }
    }
    p.stem_mix = Some(mix);
    p.handoff_bar = Some(shape[1][3].start);
    let (strategy, stages) = super::description::describe(frames, shape, n);
    let s = p.summary.as_mut()?;
    s.strategy = strategy;
    s.used_fallback = false;
    p.stages = stages;
    Some(())
}

#[cfg(test)]
pub(super) fn apply(
    p: &mut MixPlan,
    frames: &[Frame],
    shape: &Shape,
    ctx: &crate::PlanContext<'_>,
) -> Option<()> {
    let prepared = Prepared::new(p, ctx, p.summary.as_ref()?.length_bars as f32);
    apply_prepared(p, frames, shape, &prepared)
}
