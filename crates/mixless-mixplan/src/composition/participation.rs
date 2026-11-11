//! An optimistic overlap bound avoids rendering schedules that cannot win.
use super::{gains, Shape};
use crate::PlanContext;
use mixless_protocol::MixPlan;

pub(super) struct Sample {
    u: f32,
    parts: [[f32; 4]; 2],
}

pub(super) fn samples(p: &MixPlan, ctx: &PlanContext<'_>, n: f32) -> Option<Vec<Sample>> {
    (0..(n * 8.) as usize)
        .map(|i| {
            // These are exact nodes of render::apply's 1/64-bar grid and of the
            // final overlap predictor. No interpolation or new sampling clock.
            let u = (i as f32 + 0.5) / 8.;
            Some(Sample {
                u,
                parts: [
                    crate::level::unmixed_parts(ctx.outgoing, p.outgoing_source.sample(u))?,
                    if u + 0.0001 >= p.incoming_start_bar {
                        crate::level::unmixed_parts(ctx.incoming, p.incoming_source.sample(u))?
                    } else {
                        [0.; 4]
                    },
                ],
            })
        })
        .collect()
}

pub(super) fn upper(samples: &[Sample], shape: &Shape, reference: [f32; 2]) -> f32 {
    let mut overlap = 0.;
    for f in samples {
        let g = gains(shape, f.u);
        let power: [f32; 2] = std::array::from_fn(|d| {
            (0..4).map(|s| f.parts[d][s] * g[d][s].powi(2)).sum::<f32>() / reference[d]
        });
        // Final EQ and filters only attenuate these powers. The absolute
        // source floor therefore bounds participation; omit the relative mix
        // share floor, since unequal attenuation can increase that share.
        // A small rounding margin keeps floating-point routing conservative.
        if power.iter().all(|p| *p >= 0.01 * 0.999) {
            overlap += 0.125;
        }
    }
    overlap
}
