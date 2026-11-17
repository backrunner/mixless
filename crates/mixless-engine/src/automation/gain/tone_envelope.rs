use super::*;

pub(super) fn level(point: &DeckPoint, lane: usize) -> f32 {
    if lane < 3 {
        point.eq[lane] as f32 / 100. - 96.
    } else {
        point.stems.map_or(-96., |s| {
            20. * (s[lane - 3] as f32 / 1000.).max(1e-5).log10()
        })
    }
}

pub(super) fn apply(
    original: &DensePlan,
    candidate: &mut DensePlan,
    curves: &[[[f32; 8]; 2]],
    dt: f32,
    slew: f32,
) {
    let duration = original.overlap_frames as f32 / original.sample_rate as f32;
    let step = STEP as f32 / original.sample_rate as f32;
    for d in 0..2 {
        for k in 0..8 {
            let slew = if k == 0 { slew } else { slew.min(1.) };
            let mut values: Vec<f32> = original
                .points
                .iter()
                .enumerate()
                .map(|(j, p)| {
                    let sec = j as f32 * step;
                    if sec >= duration {
                        return 0.;
                    }
                    let position = sec / dt;
                    let i = (position.floor() as usize).min(curves.len() - 2);
                    let db = curves[i][d][k]
                        + (curves[i + 1][d][k] - curves[i][d][k])
                            * (position - i as f32).clamp(0., 1.);
                    let edge = slew * sec.min(duration - sec).max(0.);
                    if k == 0 {
                        return db.clamp(-edge, edge);
                    }
                    let base = level(&p.decks[d], k - 1);
                    if base <= -40. {
                        return 0.;
                    }
                    let max = if k <= 3 { 12. - base } else { -base };
                    db.clamp(-edge.min(2.), edge.min(2.).min(max.max(0.)))
                })
                .collect();
            // Shrinking magnitudes preserves neutral gaps. Where a base lane
            // moves, limit the offset's slope to the base slope as well: the
            // composed fade cannot reverse direction or create a second entry.
            limit_signed_slope(&mut values, |i: usize| {
                if k == 0 {
                    return slew * step;
                }
                let delta = (level(&original.points[i].decks[d], k - 1)
                    - level(&original.points[i - 1].decks[d], k - 1))
                .abs();
                if delta > 1e-6 {
                    (slew * step).min(delta)
                } else {
                    slew * step
                }
            });
            for (point, value) in candidate.points.iter_mut().zip(values) {
                if k == 0 {
                    point.decks[d].compensation = (value * 100.).round() as i32;
                } else {
                    point.decks[d].tone_compensation[k - 1] = (value * 100.).round() as i32;
                }
            }
        }
    }
}
