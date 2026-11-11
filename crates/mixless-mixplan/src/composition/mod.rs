//! Fit independently timed parts to the two arrangements before ranking a mix.
//! The callback only plays the resulting control curves.
mod alternatives;
mod arrival;
mod cost;
mod description;
mod evaluation;
mod evidence;
mod occupancy;
mod participation;
mod phrasing;
mod render;
mod search;
mod seeds;
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
    power_with_filters(f, g, [highpass(g[0], g[1]), highpass(g[1], g[0])])
}

fn power_with_filters(f: &Frame, g: [[f32; 4]; 2], highpass: [f32; 2]) -> [[f32; 3]; 2] {
    std::array::from_fn(|d| {
        std::array::from_fn(|b| {
            (0..4)
                .map(|s| f.bands[d][s][b] * g[d][s].powi(2))
                .sum::<f32>()
                / (1. + (highpass[d] / [70f32, 600., 5000.][b]).powi(4))
        })
    })
}

#[cfg(test)]
fn cost(frames: &[Frame], shape: &Shape, tonality: Tonality) -> Option<f32> {
    cost::Prepared::new(frames, tonality).score(shape).ok()
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
    let candidates = seeds::candidates(
        p,
        ctx,
        tonality,
        &frames,
        reference,
        n,
        manual,
        &voices,
        protected,
        handoff,
        participation.as_deref(),
        &duration_reward,
    );
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
