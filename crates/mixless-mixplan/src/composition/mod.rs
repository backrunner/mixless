//! Fit independently timed parts to the two arrangements before ranking a mix.
//! The callback only plays the resulting control curves.
mod evidence;
mod render;
#[cfg(test)]
mod tests;

use crate::PlanContext;
use evidence::{Frame, frames, voice_windows};
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
        ease((u - self.start) / (self.end - self.start).max(0.0001))
    }
}
type Shape = [[Fade; 4]; 2];

fn gains(shape: &Shape, u: f32) -> [[f32; 4]; 2] {
    std::array::from_fn(|d| {
        std::array::from_fn(|s| {
            let x = shape[d][s].at(u);
            if d == 0 { 1. - x } else { x }
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
fn cost(frames: &[Frame], shape: &Shape, harmonic: bool, protected: f32) -> Option<f32> {
    let mut cost = 0.;
    for f in frames {
        let g = gains(shape, f.u);
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
        let p = power(f, g);
        let full = |d: usize| f.bands[d].iter().flatten().sum::<f32>();
        let progress = shape[1][2].at(f.u);
        let target = (full(0) * (1. - progress) + full(1) * progress).max(1e-12);
        let heard = p.iter().flatten().sum::<f32>().max(1e-12);
        let delta = 10. * (heard / target).log10();
        if delta < -6. {
            return None;
        }
        // During a complete outgoing peak, replacing a foreground is allowed
        // only if the heard parts retain its weight (within 3 dB).
        if f.u < protected - 0.25 && heard < full(0) * 0.5 {
            return None;
        }
        cost += delta.min(0.).powi(2) + 0.2 * delta.max(0.).powi(2);
        // Bass is a short, complementary exchange; drums and melody may
        // overlap, but measured competing mid/high power has a real cost.
        for b in [1, 2] {
            cost += 4. * p[0][b].min(p[1][b]) / (p[0][b] + p[1][b]).max(1e-12);
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
pub(crate) fn arrange(p: &mut MixPlan, ctx: &PlanContext<'_>, harmonic: bool) -> Option<()> {
    let n = p.summary.as_ref()?.length_bars as f32;
    if n < 4. {
        return None;
    }
    let frames = frames(p, ctx)?;
    let voices = [
        voice_windows(&p.outgoing_source, ctx.outgoing, n, 0.125),
        voice_windows(&p.incoming_source, ctx.incoming, n, 0.125),
    ];
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
    let mut best = None;
    let mut best_cost = f32::INFINITY;
    // Separate foreground, kit and bass timings. Anchors come from the pair's
    // phrase handoff; each width is subsequently fitted to measured energy.
    for tonal in [handoff - 4., handoff, handoff + 4.] {
        let tonal = tonal.clamp(1., n - 0.5);
        for drums in [handoff - 2., handoff, handoff + 2.] {
            let drums = drums.clamp(0.5, n - 0.25);
            let mut shape = [[Fade { start: 0., end: n }; 4]; 2];
            for (part, anchor, width) in [(1, drums, 1.), (2, tonal, 2.), (3, handoff, 0.25)] {
                let end = (anchor + width).min(n);
                shape[0][part] = Fade { start: anchor, end };
                shape[1][part] = shape[0][part];
            }
            let pair = voices[0]
                .iter()
                .copied()
                .filter_map(|out| {
                    let threshold = if harmonic {
                        out.end
                    } else {
                        out.end.max(tonal).max(handoff)
                    };
                    let first = voices[1].partition_point(|f| f.start < threshold - 0.001);
                    let closest = voices[1].partition_point(|f| f.start < tonal).max(first);
                    [first, closest.saturating_sub(1).max(first), closest]
                        .into_iter()
                        .filter_map(|i| voices[1].get(i).copied())
                        .min_by(|a, b| (a.start - tonal).abs().total_cmp(&(b.start - tonal).abs()))
                        .map(|input| (out, input))
                })
                .min_by(|a, b| {
                    let distance = |pair: &(Fade, Fade)| {
                        (pair.0.start - tonal).abs() + (pair.1.start - tonal).abs()
                    };
                    distance(a).total_cmp(&distance(b))
                });
            let Some((voice_out, voice_in)) = pair else {
                continue;
            };
            shape[0][0] = voice_out;
            shape[1][0] = voice_in;
            if !harmonic {
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
            for part in [1, 2] {
                for deck in 0..2 {
                    let mut selected = shape;
                    let mut current =
                        cost(&frames, &shape, harmonic, protected).unwrap_or(f32::INFINITY);
                    for width in [0.5, 1., 2., 4.] {
                        let mut variant = shape;
                        if !harmonic && part == 2 {
                            continue;
                        }
                        variant[deck][part].end = (variant[deck][part].start + width).min(n);
                        if let Some(c) = cost(&frames, &variant, harmonic, protected)
                            .filter(|c| *c < current - 1e-4)
                        {
                            selected = variant;
                            current = c;
                        }
                    }
                    shape = selected;
                }
            }
            // The incoming drop must be revealed by its arrival. Keeping a
            // selected outgoing part over it is fine; hiding B's drop is not.
            let peak_ready = crate::drops::peaks(ctx.incoming).iter().all(|peak| {
                if peak.0 < p.t_in_b - 0.05 || peak.0 >= p.t_end_b - 0.05 {
                    return true;
                }
                let u = bar_at(&p.incoming_source, peak.0);
                shape[1].iter().all(|f| f.end <= u + 0.01)
            });
            if !peak_ready {
                continue;
            }
            if let Some(c) =
                cost(&frames, &shape, harmonic, protected).filter(|c| *c < best_cost - 1e-4)
            {
                best = Some(shape);
                best_cost = c;
            }
        }
    }
    let shape = best?;
    render::apply(p, &frames, &shape)?;
    // This is a deduction, so the caller's cheap score upper-bound remains
    // valid. An attractive nominal overlap cannot outrank its audible fit.
    p.summary.as_mut()?.score -= (best_cost / 100.).min(0.1);
    Some(())
}
