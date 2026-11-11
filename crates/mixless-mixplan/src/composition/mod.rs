//! Fit independently timed parts to the two arrangements before ranking a mix.
//! The callback only plays the resulting control curves.
mod alternatives;
mod arrival;
mod description;
mod evaluation;
mod evidence;
mod occupancy;
mod participation;
mod phrasing;
mod render;
mod search;
#[cfg(test)]
mod tests;
mod voices;

use crate::{score::Tonality, PlanContext};
use evidence::{frames, voice_windows, Frame};
use mixless_protocol::{MixPlan, Polyline};

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
    fn at(self, u: f32) -> f32 {
        if self.end <= self.start {
            return f32::from(u >= self.end);
        }
        ease((u - self.start) / (self.end - self.start))
    }
}
type Shape = [[Fade; 4]; 2];

fn gains(shape: &Shape, u: f32) -> [[f32; 4]; 2] {
    std::array::from_fn(|d| {
        std::array::from_fn(|s| {
            let x = shape[d][s].at(u);
            if d == 0 {
                1. - x
            } else {
                x
            }
        })
    })
}

fn highpass(g: [f32; 4], other: [f32; 4]) -> f32 {
    // An isolated rhythm layer starts above the other deck's kick/bass.
    20. * 12.5f32.powf((1. - g[0]) * (1. - g[2]) * (1. - g[3]) * g[1] * other[3])
}

fn power(f: &Frame, g: [[f32; 4]; 2]) -> [[f32; 3]; 2] {
    std::array::from_fn(|d| {
        std::array::from_fn(|b| {
            (0..4)
                .map(|s| f.bands[d][s][b] * g[d][s].powi(2))
                .sum::<f32>()
                / (1. + (highpass(g[d], g[1 - d]) / [70f32, 600., 5000.][b]).powi(4))
        })
    })
}

/// Relative power and masking, independent of mastering gain. The reference
/// follows the actual foreground exchange, not a line from the first sample
/// to a quiet incoming intro: that line would fade an ongoing outgoing peak.
fn cost(frames: &[Frame], shape: &Shape, tonality: Tonality) -> Option<f32> {
    let mut cost = 0.;
    let mut absent = 0.;
    let mut previous = 0.;
    // Tonal time both decks hold above -12 dB (unknown keys) or at all
    // (conflicting keys): one bar is a crossfade, not a sustained layer.
    let mut shared = 0.;
    for f in frames {
        let g = gains(shape, f.u);
        if tonality != Tonality::Compatible {
            // Only a part with measurable pitched power (-20 dB within its
            // deck) can clash. An open lane over a silent stem is not a layer.
            let sounding = |d: usize, s: usize| {
                f.bands[d][s].iter().sum::<f32>() > f.bands[d].iter().flatten().sum::<f32>() * 0.01
            };
            let tonal = |d: usize| {
                [0, 2, 3]
                    .into_iter()
                    .filter(|s| sounding(d, *s))
                    .map(|s| g[d][s])
                    .fold(0., f32::max)
            };
            // Singers never trade lines across keys that are not confirmed.
            if sounding(0, 0) && sounding(1, 0) && g[0][0] * g[1][0] > 0.0001 {
                return None;
            }
            let floor = if tonality == Tonality::Unknown {
                0.25
            } else {
                0.01
            };
            if tonal(0) > floor && tonal(1) > floor {
                shared += f.u - previous;
                if shared > 1. + 1e-4 {
                    return None;
                }
            }
        }
        if f.voice[0].min(f.voice[1]) > 0.35 && g[0][0] * g[1][0] > 0.01 {
            return None;
        }
        let p = power(f, g);
        let full = |d: usize| f.bands[d].iter().flatten().sum::<f32>();
        let progress = shape[1][2].at(f.u);
        let target = (full(0) * (1. - progress) + full(1) * progress).max(1e-12);
        let heard = p.iter().flatten().sum::<f32>().max(1e-12);
        // Do not leave a whole bar of drums between two removed melodic beds.
        // A carried singer can own that passage; natural instrumental silence
        // and an audible controlled fade are also legitimate arrangements.
        let melody = |d: usize| f.bands[d][2][1..].iter().sum::<f32>();
        let available = melody(0).max(melody(1));
        let bed = melody(0) * g[0][2].powi(2) + melody(1) * g[1][2].powi(2);
        let voice: f32 = (0..2)
            .map(|d| f.bands[d][0].iter().sum::<f32>() * g[d][0].powi(2))
            .sum();
        let foreground = |d: usize| melody(d) + f.bands[d][0].iter().sum::<f32>();
        let expected = foreground(0) * (1. - progress) + foreground(1) * progress;
        if (available > (full(0) + full(1)) * 0.05
            && bed < available * 0.0001
            && voice < available * 0.125)
            || (expected > (full(0) + full(1)) * 0.05 && bed + voice < expected * 0.25)
        {
            absent += f.u - previous;
            if absent >= 1. {
                return None;
            }
        } else {
            absent = 0.;
        }
        previous = f.u;
        let delta = 10. * (heard / target).log10();
        if delta < -6. {
            return None;
        }
        // Peak weight is checked over a complete local bar by
        // foreground::preserved after rendering the actual routes. A single
        // complementary stem-crossing sample may dip below half power without
        // losing that bar's weight; rejecting it here prevents a valid swap.
        cost += delta.min(0.).powi(2) + 0.2 * delta.max(0.).powi(2);
        // Bass is a short, complementary exchange; drums and melody may
        // overlap, but measured competing mid/high power has a real cost.
        for b in [1, 2] {
            cost += 4. * p[0][b].min(p[1][b]) / (p[0][b] + p[1][b]).max(1e-12);
        }
    }
    // A loud chord should not appear simply because a short fade minimized
    // simultaneous power. Price the audible foreground's control movement
    // in wall-clock seconds, while natural attacks at a held gain stay free.
    for pair in frames.windows(2) {
        let dt = (pair[1].sec - pair[0].sec).max(0.001);
        let (before, after) = (gains(shape, pair[0].u), gains(shape, pair[1].u));
        let total = pair[1]
            .bands
            .iter()
            .flatten()
            .flatten()
            .sum::<f32>()
            .max(1e-12);
        for d in 0..2 {
            let weight = pair[1].bands[d][2].iter().sum::<f32>() / total;
            let slope = (after[d][2] - before[d][2]).abs() / dt;
            cost += 16. * weight * slope.powi(2);
        }
    }
    Some(cost / frames.len() as f32)
}

fn bar_at(source: &Polyline, sec: f32) -> f32 {
    let i = source
        .nodes
        .partition_point(|p| p.1 < sec)
        .min(source.nodes.len() - 1);
    if i == 0 {
        return source.nodes[0].0;
    }
    let (a, b) = (source.nodes[i - 1], source.nodes[i]);
    a.0 + (b.0 - a.0) * ((sec - a.1) / (b.1 - a.1).max(1e-6)).clamp(0., 1.)
}

/// A legacy vocal carry uses the same fine rest/held-note evidence as the
/// fitted part solver, over its complete quarter-bar dry-voice release.
pub(crate) fn voice_release_after(
    t: &mixless_protocol::TrackAnalysis,
    source: &Polyline,
    n: f32,
    after: f32,
) -> Option<f32> {
    voice_windows(source, t, n, 0.25)
        .into_iter()
        .find(|f| f.start >= after - 0.001)
        .map(|f| f.start)
}

/// Return a measured arrangement when a feasible part schedule exists. Other
/// candidate arrangements still pass the same final foreground/voice checks.
pub(crate) fn arrange(
    p: &mut MixPlan,
    ctx: &PlanContext<'_>,
    tonality: Tonality,
    duration_reward: impl Fn(f32) -> f32 + Sync,
) -> Option<()> {
    let harmonic = tonality == Tonality::Compatible;
    let n = p.summary.as_ref()?.length_bars as f32;
    if n < 4. {
        return None;
    }
    let frames = frames(p, ctx)?;
    let reference = [
        crate::level::reference_power(ctx.outgoing)?,
        crate::level::reference_power(ctx.incoming)?,
    ];
    let participation = participation::samples(p, ctx, n);
    let mut voices = [
        voice_windows(&p.outgoing_source, ctx.outgoing, n, 0.125),
        voice_windows(&p.incoming_source, ctx.incoming, n, 0.125),
    ];
    // A source entrance already at a complete breath can open the voice at
    // launch. Requiring a second rest later would hide its first whole line.
    // Natural voice overlap is still checked against the outgoing source.
    let launch = p.incoming_start_bar;
    if crate::vocals::safe_entry(ctx.incoming, p.incoming_source.sample(launch)) {
        voices[1].push(Fade {
            start: launch,
            end: launch,
        });
        voices[1].sort_by(|a, b| a.start.total_cmp(&b.start).then(a.end.total_cmp(&b.end)));
    }
    let manual = crate::constraints::user_range(ctx.cues_out, ctx.outgoing.sample_rate).is_some();
    let protected = if manual {
        0.
    } else {
        crate::drops::peaks(ctx.outgoing)
            .into_iter()
            .filter(|p0| p0.0 < p.t_out_a - 0.05 && p0.1 > p.t_in_a + 0.05)
            .map(|p0| bar_at(&p.outgoing_source, p0.1))
            .fold(0f32, f32::max)
    };
    let handoff = p.handoff_bar?.clamp(0.5, n - 0.25);
    let costs = search::Costs::new(&frames, tonality);
    let mut windows = crate::melody::Search::new(ctx, p, n);
    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut consider = |shape: Shape| {
        let key = search::key(&shape);
        if !seen.insert(key) {
            return;
        }
        // Count the audible overlap, not a reserved playback window.
        if let Some(c) = costs.get(&shape) {
            let base = (c / 100.).min(0.1);
            let upper = participation.as_ref().map_or(n, |samples| {
                participation::upper(samples, &shape, reference)
            });
            let lower_cost = base + (duration_reward(n) - duration_reward(upper)).max(0.);
            candidates.push(evaluation::Candidate {
                shape,
                base,
                lower_cost,
            });
        }
    };
    // Separate foreground, kit and bass timings. Anchors come from the pair's
    // phrase handoff; each width is subsequently fitted to measured energy.
    for tonal in [handoff - 4., handoff, handoff + 4., protected] {
        let tonal = tonal.clamp(1., n - 0.5);
        for drums in [
            handoff - 4.,
            handoff - 2.,
            handoff,
            handoff + 2.,
            handoff + 4.,
        ] {
            let drums = drums.clamp(0.5, n - 0.25);
            let mut shape = [[Fade { start: 0., end: n }; 4]; 2];
            for (part, anchor, width) in [(1, drums, 1.), (2, tonal, 2.), (3, handoff, 0.25)] {
                let end = (anchor + width).min(n);
                shape[0][part] = Fade { start: anchor, end };
                shape[1][part] = shape[0][part];
            }
            for (voice_out, voice_in) in voices::pairs(&voices, tonal, handoff, harmonic, manual) {
                let mut shape = shape;
                shape[0][0] = voice_out;
                shape[1][0] = voice_in;
                if !harmonic && manual {
                    // Unknown/different keys: every tonal part of A must be out
                    // before any tonal part of B is exposed. Drums can still ride.
                    let gate = voice_out.end.max(tonal).max(handoff);
                    if gate > n - 0.25 || voice_in.start < gate {
                        continue;
                    }
                    for part in [2, 3] {
                        shape[0][part] = Fade {
                            start: (gate - 1.).max(0.25),
                            end: gate,
                        };
                        shape[1][part] = Fade {
                            start: gate,
                            end: (gate + 1.).min(n),
                        };
                    }
                }
                // Coordinate descent, with independent widths on the two decks.
                // Voice fades stay inside measured silent breaths and bass stays
                // complementary, so neither can be stretched into an unsafe line.
                for part in [1] {
                    for deck in 0..2 {
                        let mut selected = shape;
                        let mut current = costs.get(&shape).unwrap_or(f32::INFINITY);
                        for width in [0.5, 1., 2., 4.] {
                            let mut variant = shape;
                            if !harmonic && part == 2 && deck == 0 {
                                variant[deck][part].start =
                                    (variant[deck][part].end - width).max(0.);
                            } else {
                                variant[deck][part].end =
                                    (variant[deck][part].start + width).min(n);
                            }
                            if let Some(c) = costs.get(&variant).filter(|c| *c < current - 1e-4) {
                                selected = variant;
                                current = c;
                            }
                        }
                        shape = selected;
                    }
                }
                let shapes = if manual {
                    vec![shape]
                } else {
                    phrasing::shapes(
                        &mut windows,
                        p,
                        ctx,
                        shape,
                        &voices[1],
                        tonality,
                        protected,
                        n,
                    )
                };
                for shape in shapes {
                    consider(shape);
                }
            }
        }
    }
    for shape in alternatives::shapes(&mut windows, p, ctx, &voices, tonality, n) {
        consider(shape);
    }
    let (best, best_cost) = evaluation::choose(
        p,
        ctx,
        &frames,
        reference,
        n,
        manual,
        &candidates,
        &duration_reward,
    )?;
    *p = best;
    // This is a deduction, so the caller's cheap score upper-bound remains
    // valid. An attractive nominal overlap cannot outrank its audible fit.
    p.summary.as_mut()?.score -= best_cost;
    Some(())
}
