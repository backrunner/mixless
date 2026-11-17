//! Bounded two-deck gain fitting. Never changes an arrangement lane.
use super::*;
use meter::Reading;

fn db_ratio(a: f64, b: f64) -> f64 {
    10. * (a.max(1e-15) / b.max(1e-15)).log10()
}
fn error(db: f64) -> f64 {
    (db.abs() - 1.).max(0.).powi(2)
}

pub(super) fn target(reference: &Reading, band: usize) -> f64 {
    reference[0].power[band] + reference[1].power[band]
}

pub(super) fn loss(power: [f64; 4], reference: &Reading) -> f64 {
    let broadband = target(reference, 0);
    if broadband < 1e-6 {
        return 0.;
    }
    let mut loss = error(db_ratio(power[0], broadband));
    for (band, value) in power.into_iter().enumerate().skip(1) {
        let target = target(reference, band);
        // Spectral holes do not authorize amplification. Penalize excess in
        // retained bands so a missing bass stem cannot turn into louder vocals.
        if target > 1e-8 {
            loss += 0.5 * (db_ratio(value, target) - 1.).max(0.).powi(2);
        }
    }
    loss
}

fn predicted(base: &Reading, gain: [f32; 2]) -> [f64; 4] {
    let g = gain.map(|db| 10f64.powf(db as f64 / 20.));
    std::array::from_fn(|b| {
        let a = base[0].power[b];
        let c = base[1].power[b];
        let cross = base[2].power[b] - a - c;
        (a * g[0].powi(2) + c * g[1].powi(2) + cross * g[0] * g[1]).max(0.)
    })
}

pub(super) fn point(
    base: &Reading,
    reference: &Reading,
    routing: [f32; 2],
    manual: [f32; 2],
) -> [f32; 2] {
    let sum = target(reference, 0);
    if sum < 1e-6 {
        return [0.; 2];
    }
    let strongest = routing.into_iter().fold(0f32, f32::max).max(0.01);
    let a = base[0].power[0];
    let b = base[1].power[0];
    let covariance = base[2].power[0] - a - b;
    let cancellation = covariance < -0.1 * (a + b);
    let excess = db_ratio(base[2].power[0], sum) > 1.;
    let limits: [(f32, f32); 2] = std::array::from_fn(|d| {
        let authority = (routing[d] / strongest).powi(2);
        let p = base[d].power[0];
        let r = reference[d].power[0];
        if routing[d] < 0.01 || p < 1e-6 || r < 1e-6 || p < r * 0.01 {
            return (0., 0.);
        }
        let boost = if cancellation {
            0.
        } else {
            ((db_ratio(r, p) - 1.).max(0.) as f32)
                .min(6. * authority)
                .min((12. - manual[d]).max(0.))
        };
        let cut = if excess {
            (-3. * authority).max(-12. - manual[d])
        } else {
            0.
        };
        (cut.min(0.), boost)
    });
    let score = |gain: [f32; 2]| {
        loss(predicted(base, gain), reference)
            + 0.02 * gain.iter().map(|g| f64::from(*g).powi(2)).sum::<f64>()
    };
    let mut best = [0.; 2];
    let mut cost = score(best);
    // Small bounded coordinate search over gain only, with an explicit zero
    // candidate. No cue, strategy, crossfader or stem-envelope search occurs.
    for _ in 0..3 {
        for d in 0..2 {
            let (low, high) = limits[d];
            for i in (low * 4.).ceil() as i32..=(high * 4.).floor() as i32 {
                let mut candidate = best;
                candidate[d] = i as f32 / 4.;
                let next = score(candidate);
                if next + 1e-9 < cost {
                    best = candidate;
                    cost = next;
                }
            }
        }
    }
    best
}

pub(super) fn acceptable(base: &[Reading], reference: &[Reading], result: &[Reading]) -> bool {
    validate(base, reference, result, false)
}

pub(super) fn spectral_loss(power: [f64; 4], reference: &Reading) -> f64 {
    let mut total = loss(power, reference);
    for band in 1..4 {
        let wanted = target(reference, band);
        if wanted > 1e-8 {
            total += 0.5 * (-db_ratio(power[band], wanted) - 1.).max(0.).powi(2);
        }
    }
    total
}

pub(super) fn validate(
    base: &[Reading],
    reference: &[Reading],
    result: &[Reading],
    spectral: bool,
) -> bool {
    if base.len() != reference.len()
        || base.len() != result.len()
        || !stable_output_steps(base, result)
    {
        return false;
    }
    let objective = if spectral { spectral_loss } else { loss };
    let mut before = 0.;
    let mut after = 0.;
    let mut output_before = 0.;
    let mut output_after = 0.;
    for ((b, r), c) in base.iter().zip(reference).zip(result) {
        before += objective(b[2].power, r);
        after += objective(c[2].power, r);
        // Preserve the user's master voicing in the comparison. This target
        // uses the baseline transfer; acceptance uses the re-rendered output.
        let mut output_reference = *r;
        for band in 0..4 {
            output_reference[0].power[band] =
                target(r, band) * b[4].power[band] / b[2].power[band].max(1e-15);
            output_reference[1].power[band] = 0.;
        }
        output_before += objective(b[4].power, &output_reference);
        output_after += objective(c[4].power, &output_reference);
        for d in [0, 1, 4] {
            if c[d].reduction > b[d].reduction + 0.5 {
                return false;
            }
        }
        // Check the real final output as well as the pre-master graph. More
        // limiting must not masquerade as successful loudness compensation.
        if c[4].peak > b[4].peak.max(10f64.powf(-1. / 20.)) * 1.01 {
            return false;
        }
        if b[3].power[0] > 1e-6 && c[3].power[0] > 1e-6 && b[4].power[0] > 1e-6 {
            let transfer = (c[4].power[0] / c[3].power[0]) / (b[4].power[0] / b[3].power[0]);
            if 10. * transfer.max(1e-15).log10() < -0.5 {
                return false;
            }
            let expected_short = (r[0].short + r[1].short) / b[2].short.max(1e-15);
            if c[4].short / b[4].short.max(1e-15) > expected_short.max(1.) * 10f64.powf(1. / 10.) {
                return false;
            }
            // A nonlinear send/master must not create a new output surge.
            for band in 0..4 {
                if b[4].power[band] < 1e-8 || target(r, band) < 1e-8 {
                    continue;
                }
                let expected = target(r, band) / b[2].power[band].max(1e-15);
                let output_ratio = c[4].power[band] / b[4].power[band].max(1e-15);
                if output_ratio > expected.max(1.) * 10f64.powf(1. / 10.) {
                    return false;
                }
            }
        }
    }
    before > 0. && after < before * 0.99 && output_after <= output_before
}

fn stable_output_steps(base: &[Reading], result: &[Reading]) -> bool {
    base.windows(2)
        .zip(result.windows(2))
        .all(|(b, c)| stable_step(&b[0][4], &b[1][4], &c[0][4], &c[1][4]))
}

pub(super) fn stable_output_windows(base: &[meter::Metric], result: &[meter::Metric]) -> bool {
    // Four 50 ms offsets of the same 200 ms difference catch changes hidden
    // between the fitting grid's samples. Comparing only whole-transition
    // maxima, or only one grid phase, can miss a new localized energy jump.
    base.len() == result.len()
        && base
            .windows(5)
            .zip(result.windows(5))
            .all(|(b, c)| stable_step(&b[0], &b[4], &c[0], &c[4]))
}

pub(super) fn stable_output(base: &render::Output, result: &render::Output) -> bool {
    let lag = base.step_lag;
    lag > 0 && lag == result.step_lag && base.bands.len() == result.bands.len()
        && stable_output_windows(&base.levels, &result.levels)
        // Band power already advances every audio block. Check every offset
        // without repeating crossover filtering or adding audio-thread work.
        && base.bands.windows(lag + 1).zip(result.bands.windows(lag + 1)).all(|(b, c)| {
            (0..3).all(|k| stable_ratio(b[0][k], b[lag][k], c[0][k], c[lag][k]))
        })
}

fn stable_step(
    b0: &meter::Metric,
    b1: &meter::Metric,
    c0: &meter::Metric,
    c1: &meter::Metric,
) -> bool {
    // Slow controls alone do not bound output changes: a boosted source
    // can suddenly dominate when a stem returns or the crossfader moves.
    // Preserve the baseline's natural transients; allow at most the same
    // 1 dB deadband of additional contrast between measured 400 ms windows
    // (200 ms hops), reserving 0.25 dB for the different window alignment
    // and carried DSP state in continuous playback. Include short-term
    // output, not only the fitted sum.
    b0.power
        .into_iter()
        .chain([b0.short])
        .zip(b1.power.into_iter().chain([b1.short]))
        .zip(c0.power.into_iter().chain([c0.short]))
        .zip(c1.power.into_iter().chain([c1.short]))
        .all(|(((before, after), from), to)| stable_ratio(before, after, from, to))
}

fn stable_ratio(before: f64, after: f64, from: f64, to: f64) -> bool {
    // Ratios in effectively silent bands are not useful evidence.
    [before, after, from, to].into_iter().any(|p| p < 1e-8)
        || db_ratio(to, from).abs() <= db_ratio(after, before).abs() + 0.75
}
