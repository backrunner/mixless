//! Onset autocorrelation and phase refinement.
use super::*;

pub(super) fn tempo(frames: &[Frame], duration: f32) -> (f32, f32, f32) {
    if frames.len() < 128 {
        return (120., 0., 0.);
    }
    let mut flux: Vec<_> = frames.iter().map(|f| f.onset).collect();
    let dt = HOP as f32 / SR;
    // Onset flux is all-positive, so the raw autocorrelation is dominated by
    // the DC component and favours metrical aliases. Centre it first.
    let mean = flux.iter().sum::<f32>() / flux.len() as f32;
    flux.iter_mut().for_each(|v| *v -= mean);
    let norm = flux.iter().map(|v| v * v).sum::<f32>();
    if norm < 1e-6 {
        return (120., 0., 0.);
    }
    let corr = |lag: usize| {
        flux.iter()
            .skip(lag)
            .zip(&flux)
            .map(|(a, b)| a * b)
            .sum::<f32>()
            / (norm * (1. - lag as f32 / flux.len() as f32)).max(1e-6)
    };
    let min = (60. / 180. / dt) as usize;
    let max = (60. / 70. / dt).ceil() as usize;
    // Compute each autocorrelation once, including across phrase probes and
    // the 2x/4x harmonic lags the comb scorer below consults. Never narrow
    // the range below `max + 1`: the peak search and interpolation index it
    // directly.
    let hi = (4 * max + 1).min((flux.len() / 2).max(max + 1));
    let correlations: Vec<_> = (min - 1..=hi).map(corr).collect();
    let corr_at = |lag: usize| {
        lag.checked_sub(min - 1)
            .and_then(|i| correlations.get(i))
            .copied()
            .unwrap_or(0.)
    };
    let comb = |bpm: f32| {
        let lag = (60. / bpm / dt).round() as usize;
        corr_at(lag) + 0.6 * corr_at(2 * lag) + 0.4 * corr_at(4 * lag)
    };
    let best = (min..=max)
        .max_by(|a, b| correlations[*a - min + 1].total_cmp(&correlations[*b - min + 1]))
        .unwrap_or(min);
    let at = best - min + 1;
    let (left, mid, right) = (correlations[at - 1], correlations[at], correlations[at + 1]);
    let offset = (0.5 * (left - right) / (left - 2. * mid + right).min(-1e-6)).clamp(-0.5, 0.5);
    let center = 60. / ((best as f32 + offset) * dt);
    // Search plausible metrical aliases as well as the largest lag peak. A
    // dotted-note autocorrelation (e.g. 116 vs 174) is not a tempo change.
    let mut centers = vec![center];
    for factor in [0.5, 2., 2. / 3., 1.5, 0.75, 4. / 3.] {
        let candidate = center * factor;
        if (70. ..=180.).contains(&candidate) {
            centers.push(candidate);
        }
    }
    // A true tempo still correlates at its harmonic lags; a 3/4 or 4/3 alias
    // does not. Gate and rank candidates on that comb evidence.
    let best_comb = centers
        .iter()
        .flat_map(|&center| (-12..=12).map(move |step| center + step as f32 * 0.05))
        .filter(|candidate| (70. ..=180.).contains(candidate))
        .map(comb)
        .fold(0f32, f32::max);
    let mut bpm = center;
    let mut fit = 0.;
    let mut phase = 0.;
    let mut selected_mid = mid;
    for center in centers {
        for step in -12..=12 {
            let candidate = center + step as f32 * 0.05;
            if !(70. ..=180.).contains(&candidate) {
                continue;
            }
            let period = 60. / candidate;
            let strength = comb(candidate);
            if strength < best_comb * 0.45 {
                continue;
            }
            let mut histogram = [0.; 64];
            for f in frames {
                let p = ((f.time / period).fract() * 64.) as usize;
                histogram[p.min(63)] += f.onset;
            }
            let weight = 0.5 + 0.5 * strength / best_comb.max(1e-6);
            for j in 0..64 {
                // A fixed time window treats 87 and 174 BPM equally. A fixed
                // number of phase bins made the fast grid's tolerance half as
                // wide and rejected real DnB kick/snare onsets as "off grid".
                let radius = (0.022 / period * 64.).max(1.);
                let score = (-(radius.ceil() as i32)..=radius.ceil() as i32)
                    .map(|offset| {
                        histogram[(j as i32 + offset).rem_euclid(64) as usize]
                            * (1. - (offset as f32).abs() / (radius + 1.)).max(0.)
                    })
                    .sum::<f32>()
                    * weight;
                // Keep the original metrical interpretation for near-ties.
                if score > fit * 1.005 {
                    fit = score;
                    bpm = candidate;
                    phase = (j as f32 + 0.5) / 64. * period;
                    // Scale the comb evidence back to a single-lag magnitude:
                    // a pick held up only by harmonic lags reports less
                    // periodic support than its fundamental correlation.
                    selected_mid = strength * 0.5;
                }
            }
        }
    }
    // Autocorrelation magnitude depends on arrangement and transient density;
    // it is not a probability. Use prominence above other lags, then verify
    // that the measured phase beats displaced control grids locally.
    let mut background = correlations.clone();
    background.sort_by(f32::total_cmp);
    let median = background[background.len() / 2];
    let mut deviations: Vec<_> = background.iter().map(|v| (v - median).abs()).collect();
    deviations.sort_by(f32::total_cmp);
    let mad = deviations[deviations.len() / 2];
    let periodicity = ((selected_mid - median) / (6. * mad).max(0.025)).clamp(0., 1.);
    let confidence =
        periodicity.min(alignment_confidence(frames, bpm, phase)) * (duration / 16.).min(1.);
    (bpm, phase, confidence)
}

/// Contrast the proposed pulse train with quarter-beat displaced controls.
/// Offbeat drums remain valid musical evidence; diffuse/noisy energy cannot
/// acquire confidence merely by filling every beat window.
pub(super) fn alignment_confidence(frames: &[Frame], bpm: f32, phase: f32) -> f32 {
    let period = 60. / bpm;
    let mut energy = [[0f32; 3]; 2];
    for f in frames {
        for (i, shift) in [0., -0.25, 0.25].into_iter().enumerate() {
            let p = (f.time - phase - shift * period).rem_euclid(period);
            if p.min(period - p) <= 0.035 {
                energy[0][i] += f.onset;
                energy[1][i] += f.low_onset;
            }
        }
    }
    // Dense hats/rolls may mask the broad-band pulse while low-frequency
    // transients remain locked. Require phase contrast in either measured
    // onset band; the independent autocorrelation/accent gates still apply.
    energy
        .into_iter()
        .map(|energy| {
            if energy[0] <= 1e-6 {
                return 0.;
            }
            let control = (energy[1] + energy[2]) * 0.5;
            ((energy[0] - control) / energy[0] / 0.65).clamp(0., 1.)
        })
        .fold(0., f32::max)
}

/// Confidence of an existing clock. The bar pattern may repeat every two beats
/// in breakbeat music; evaluate those aliases without changing tempo or phase.
pub(super) fn pulse_confidence(frames: &[Frame], bpm: f32, phase: f32) -> f32 {
    if frames.len() < 128 {
        return 0.;
    }
    let mean = frames.iter().map(|f| f.onset).sum::<f32>() / frames.len() as f32;
    let norm = frames.iter().map(|f| (f.onset - mean).powi(2)).sum::<f32>();
    if norm <= 1e-6 {
        return 0.;
    }
    let dt = HOP as f32 / SR;
    let lo = (60. / 180. / dt) as usize;
    let hi = (60. / 70. / dt).ceil() as usize;
    let correlations: Vec<_> = (lo..=hi)
        .map(|lag| {
            frames
                .iter()
                .skip(lag)
                .zip(frames)
                .map(|(a, b)| (a.onset - mean) * (b.onset - mean))
                .sum::<f32>()
                / (norm * (1. - lag as f32 / frames.len() as f32)).max(1e-6)
        })
        .collect();
    let supported = [0.5, 1., 2.]
        .into_iter()
        .filter_map(|factor| {
            let lag = (60. / (bpm * factor) / dt).round() as usize;
            lag.checked_sub(lo)
                .and_then(|i| correlations.get(i))
                .copied()
        })
        .fold(0., f32::max);
    let mut sorted = correlations.clone();
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let mut deviations: Vec<_> = sorted.iter().map(|v| (v - median).abs()).collect();
    deviations.sort_by(f32::total_cmp);
    let mad = deviations[deviations.len() / 2];
    ((supported - median) / (6. * mad).max(0.025))
        .clamp(0., 1.)
        .min(alignment_confidence(frames, bpm, phase))
}
