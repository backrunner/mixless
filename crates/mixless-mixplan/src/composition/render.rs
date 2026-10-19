use super::{ease, gains, highpass, power, Frame, Shape};
use mixless_protocol::{MixPlan, Polyline, StemMix};

fn db(x: f32) -> f32 {
    if x <= 0.000016 {
        -96.
    } else {
        (20. * x.log10()).clamp(-96., 12.)
    }
}

pub(super) fn apply(p: &mut MixPlan, frames: &[Frame], shape: &Shape) -> Option<()> {
    let n = p.summary.as_ref()?.length_bars as f32;
    let last = p.clock.nodes.last()?.0.max(n);
    let mut points: Vec<f32> = (0..=n as usize * 64).map(|i| i as f32 / 64.).collect();
    points.extend(shape.iter().flatten().flat_map(|f| [f.start, f.end]));
    points.sort_by(f32::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let mut mix = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    let opening = shape[1]
        .iter()
        .map(|f| f.start)
        .fold(n, f32::min)
        .clamp(0.125, 0.5);
    let closing = shape[0]
        .iter()
        .map(|f| f.end)
        .fold(0f32, f32::max)
        .min(n - 0.125);
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
                    .max(-remaining);
                eq[d][b].nodes.push((u, cut));
                previous[d][b] = cut;
            }
        }
        let x = if u < opening {
            -1. + ease(u / opening)
        } else if u > closing {
            ease((u - closing) / (n - closing))
        } else {
            0.
        };
        l.xfader.nodes.push((u, x));
        for d in 0..2 {
            // Factor the desired part levels into a channel level and stem
            // ratios. The crossfader stays centred during the audible mix;
            // compensate the engine's square-root law exactly, not sin/cos.
            let level = g[d].iter().copied().fold(0f32, f32::max);
            let fader = (if d == 0 { (1. - x) / 2. } else { (1. + x) / 2. }).sqrt();
            let gain = if level == 0. {
                -96.
            } else {
                db(level / fader.max(0.0001))
            };
            let lane = if d == 0 { &mut l.gain_a } else { &mut l.gain_b };
            lane.nodes.push((u, gain));
            let stems = if d == 0 {
                &mut mix.outgoing
            } else {
                &mut mix.incoming
            };
            for s in 0..4 {
                let ratio = if level > 0. {
                    g[d][s] / level
                } else {
                    f32::from(d == 1 && u >= n)
                };
                stems[s].nodes.push((u, ratio));
            }
        }
        prev_sec = sec;
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
