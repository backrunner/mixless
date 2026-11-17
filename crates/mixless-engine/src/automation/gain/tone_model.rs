use super::*;

pub(super) fn ratio(a: f64, b: f64) -> f64 {
    10. * (a.max(1e-15) / b.max(1e-15)).log10()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn solve(
    original: &Point,
    current: &Point,
    dry: &Reading,
    base: &Reading,
    reference: &Reading,
    probes: &[Probe],
    at: usize,
    manual: [f32; 2],
) -> [[f32; 8]; 2] {
    let mut initial = [[0.; 8]; 2];
    for d in 0..2 {
        initial[d][0] = current.decks[d].compensation as f32 / 100.;
    }
    let mut limits = [[(0., 0.); 8]; 2];
    let mut derivative = [[[0.; 4]; 8]; 2];
    let (a, b) = xfader_gains(original.xf as f32 / 500. - 1., XfCurve::EqualPower, false);
    let route = [
        a * original.decks[0].fader as f32 / 1000.,
        b * original.decks[1].fader as f32 / 1000.,
    ];
    let strongest = route[0].max(route[1]).max(0.01);
    for d in 0..2 {
        let authority = (route[d] / strongest).powi(2);
        if route[d] < 0.01 || base[d].power[0] < 1e-6 {
            continue;
        }
        // GAIN may be reduced to make room for a more selective correction;
        // a tone refinement does not acquire new broadband boost authority.
        limits[d][0] = (initial[d][0].min(0.), initial[d][0].max(0.));
        for b in 0..4 {
            derivative[d][0][b] = 1.;
        }
        for probe in probes {
            let lane = probe.lane;
            let level = envelope::level(&original.decks[d], lane);
            if level <= -40. {
                continue;
            }
            let mut need = 0f64;
            let mut excess = false;
            let mut audible = false;
            for band in 0..4 {
                let value = base[d].power[band] - probe.readings[at][d].power[band];
                derivative[d][lane + 1][band] =
                    ratio(base[d].power[band], probe.readings[at][d].power[band]);
                // A negative response indicates cancellation/nonlinearity,
                // not evidence that this component should be amplified.
                if value <= base[d].power[band] * 0.01 || base[d].power[band] < 1e-8 {
                    continue;
                }
                audible = true;
                let covariance = base[2].power[band] - base[0].power[band] - base[1].power[band];
                if covariance >= -0.1 * (base[0].power[band] + base[1].power[band]) {
                    need = need
                        .max((ratio(reference[d].power[band], dry[d].power[band]) - 1.).max(0.));
                }
                excess |= ratio(base[2].power[band], fit::target(reference, band)) > 1.;
            }
            if !audible {
                continue;
            }
            let max = if lane < 3 { 12. - level } else { -level };
            limits[d][lane + 1] = (
                if excess { -2. * authority } else { 0. },
                (need as f32).min(2. * authority).min(max.max(0.)),
            );
        }
    }
    let score = |x: [[f32; 8]; 2]| {
        for d in 0..2 {
            // Bound the complete gain of every stem/band path, rather than
            // independently spending the budget on GAIN, EQ and stems.
            for band in 1..4 {
                for stem in 4..8 {
                    let path = [x[d][0], x[d][band], x[d][stem]];
                    if path.iter().map(|v| v.max(0.)).sum::<f32>() > 6.01
                        || path.iter().map(|v| v.min(0.)).sum::<f32>() < -3.01
                        || manual[d] + x[d][0] > 12.01
                        || manual[d] + x[d][0] < -12.01
                    {
                        return f64::INFINITY;
                    }
                }
            }
        }
        let p: [[f64; 4]; 2] = std::array::from_fn(|d| {
            std::array::from_fn(|b| {
                (base[d].power[b]
                    * 10f64.powf(
                        (0..8)
                            .map(|k| derivative[d][k][b] * f64::from(x[d][k] - initial[d][k]))
                            .sum::<f64>()
                            / 10.,
                    ))
                .max(1e-15)
            })
        });
        let sum = std::array::from_fn(|b| {
            let cross = base[2].power[b] - base[0].power[b] - base[1].power[b];
            let scale = ((p[0][b] / base[0].power[b].max(1e-15))
                * (p[1][b] / base[1].power[b].max(1e-15)))
            .sqrt();
            (p[0][b] + p[1][b] + cross * scale).max(1e-15)
        });
        // Leave one 0.25 dB search step below the renderer's 1 dB excess
        // guard. Crossover leakage and time-varying DSP are not exact in this
        // local response model. Already-excessive bands remain constrained
        // by the measured starting level while controls exchange gain budget.
        for b in 1..4 {
            let target = fit::target(reference, b);
            if target > 1e-8 && sum[b] > base[2].power[b].max(target) * 10f64.powf(0.75 / 10.) {
                return f64::INFINITY;
            }
        }
        fit::spectral_loss(sum, reference)
            + x.iter()
                .map(|deck| {
                    deck.iter()
                        .enumerate()
                        // Prefer the isolated source over EQ affecting every
                        // source in that band. All controls still share one loss.
                        .map(|(k, v)| {
                            f64::from(*v).powi(2) * if (1..4).contains(&k) { 0.16 } else { 0.02 }
                        })
                        .sum::<f64>()
                })
                .sum::<f64>()
    };
    let mut best = initial;
    let mut cost = score(best);
    for _ in 0..3 {
        for d in 0..2 {
            for k in 0..8 {
                let (low, high) = limits[d][k];
                for step in (low * 4.).ceil() as i32..=(high * 4.).floor() as i32 {
                    let mut candidate = best;
                    candidate[d][k] = step as f32 / 4.;
                    let next = score(candidate);
                    if next + 1e-9 < cost {
                        best = candidate;
                        cost = next;
                    }
                }
            }
            // A fully spent gain budget needs a simultaneous transfer. A
            // one-coordinate search cannot move from a broadband cut to a
            // selective stem cut without first exceeding the shared bound.
            for k in 1..8 {
                for step in [-0.5, -0.25, 0.25, 0.5] {
                    let mut candidate = best;
                    candidate[d][0] += step;
                    candidate[d][k] -= step;
                    if [0, k].iter().any(|i| {
                        candidate[d][*i] < limits[d][*i].0 || candidate[d][*i] > limits[d][*i].1
                    }) {
                        continue;
                    }
                    let next = score(candidate);
                    if next + 1e-9 < cost {
                        best = candidate;
                        cost = next;
                    }
                }
            }
        }
    }
    best
}
