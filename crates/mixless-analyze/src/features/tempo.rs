//! Onset autocorrelation and phase refinement.
use super::*;

pub(super) const MIN_BPM: f32 = 70.;
pub(super) const MAX_BPM: f32 = 240.;

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
    let min = (60. / MAX_BPM / dt) as usize;
    let max = (60. / MIN_BPM / dt).ceil() as usize;
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
        let lag = 60. / bpm / dt;
        let at = |lag: f32| {
            let lo = lag.floor() as usize;
            let w = lag.fract();
            corr_at(lo) * (1. - w) + corr_at(lo + 1) * w
        };
        // Interpolate each harmonic's actual lag. Multiplying a rounded
        // fundamental accumulates the rounding error: 175 BPM has lags
        // 29.5/59/118 frames, not 30/60/120. The latter rewards dotted aliases.
        at(lag) + 0.6 * at(2. * lag) + 0.4 * at(4. * lag)
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
        if (MIN_BPM..=MAX_BPM).contains(&candidate) {
            centers.push(candidate);
        }
    }
    // A true tempo still correlates at its harmonic lags; a 3/4 or 4/3 alias
    // does not. Gate and rank candidates on that comb evidence.
    let best_comb = centers
        .iter()
        .flat_map(|&center| (-12..=12).map(move |step| center + step as f32 * 0.05))
        .filter(|candidate| (MIN_BPM..=MAX_BPM).contains(candidate))
        .map(comb)
        .fold(0f32, f32::max);
    let mut bpm = center;
    let mut fit = 0.;
    let mut phase = 0.;
    let mut selected_mid = mid;
    for center in centers {
        for step in -12..=12 {
            let candidate = center + step as f32 * 0.05;
            if !(MIN_BPM..=MAX_BPM).contains(&candidate) {
                continue;
            }
            let period = 60. / candidate;
            let strength = comb(candidate);
            if strength < best_comb * 0.45 {
                continue;
            }
            let weight = strength / best_comb.max(1e-6);
            let (score, j) = fold(frames, candidate);
            // Keep the original metrical interpretation for near-ties.
            if score * weight > fit * 1.005 {
                fit = score * weight;
                bpm = candidate;
                phase = (j as f32 + 0.5) / 64. * period;
                // Scale the comb evidence back to a single-lag magnitude:
                // a pick held up only by harmonic lags reports less
                // periodic support than its fundamental correlation.
                selected_mid = strength * 0.5;
            }
        }
    }
    // The 0.05 BPM search above leaves a residual period error that a whole
    // track accumulates: 0.05 BPM over 650 beats is a fifth of a beat, so the
    // grid drifts off the later phrases and their local alignment fails.
    // Over a long excerpt, pick the period that folds onsets most sharply.
    if duration >= 60. {
        (bpm, phase) = refine(frames, bpm, phase);
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

/// Onset weight above a uniform phase background, and its best phase bin.
pub(super) fn fold(frames: &[Frame], bpm: f32) -> (f32, usize) {
    let period = 60. / bpm;
    let mut histogram = [0.; 64];
    for f in frames {
        let p = ((f.time / period).fract() * 64.) as usize;
        histogram[p.min(63)] += f.onset;
    }
    // A fixed time window treats 87 and 174 BPM equally. A fixed number of
    // phase bins made the fast grid's tolerance half as wide and rejected
    // real DnB kick/snare onsets as "off grid".
    let radius = (0.022 / period * 64.).max(1.);
    let mut best = (0f32, 0);
    for j in 0..64 {
        let score = (-(radius.ceil() as i32)..=radius.ceil() as i32)
            .map(|offset| {
                histogram[(j as i32 + offset).rem_euclid(64) as usize]
                    * (1. - (offset as f32).abs() / (radius + 1.)).max(0.)
            })
            .sum::<f32>();
        if score > best.0 {
            best = (score, j);
        }
    }
    // A faster grid has more attack windows per second and therefore catches
    // more unrelated flux. Subtract the uniform-phase expectation before
    // comparing different periods; otherwise extending the BPM range rewards
    // dense hats and dotted-note aliases simply for being faster.
    let kernel: f32 = (-(radius.ceil() as i32)..=radius.ceil() as i32)
        .map(|offset| (1. - (offset as f32).abs() / (radius + 1.)).max(0.))
        .sum();
    best.0 = (best.0 - histogram.iter().sum::<f32>() * kernel / 64.).max(0.);
    best
}

/// Fine period search within ±0.3 BPM. The coarse fit stays unless a finer
/// period folds the onsets at least 5% more sharply: a grid that already
/// fits must not move by a few milliseconds for a marginal gain.
fn refine(frames: &[Frame], bpm: f32, phase: f32) -> (f32, f32) {
    let mut best = (bpm, fold(frames, bpm).0, phase);
    let base = best.1;
    for step in -150..=150 {
        let candidate = bpm + step as f32 * 0.002;
        let (score, j) = fold(frames, candidate);
        if score > best.1.max(base * 1.05) {
            best = (candidate, score, (j as f32 + 0.5) / 64. * 60. / candidate);
        }
    }
    (best.0, best.2)
}

/// Sub-150 Hz energy needs most of the 93 ms analysis window to build up, so
/// its rise peaks about 30 ms after the broad-band flux of the same kick,
/// measured consistently from 100 to 180 BPM.
pub(super) const LOW_ONSET_LAG: f32 = 0.03;

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
            }
            let p = (p - LOW_ONSET_LAG).rem_euclid(period);
            if p.min(period - p) <= 0.035 {
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
    let lo = (60. / MAX_BPM / dt) as usize;
    let hi = (60. / MIN_BPM / dt).ceil() as usize;
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
    // Interpolate between integer lags: at 175 BPM the beat is 29.5 frames,
    // and rounding to 30 reads the shoulder of a narrow kick peak.
    let at = |lag: f32| {
        let i = (lag - lo as f32).floor();
        if i < 0. || i as usize + 1 >= correlations.len() {
            return None;
        }
        let w = lag - lo as f32 - i;
        Some(correlations[i as usize] * (1. - w) + correlations[i as usize + 1] * w)
    };
    let supported = [0.5, 1., 2.]
        .into_iter()
        .filter_map(|factor| at(60. / (bpm * factor) / dt))
        .fold(0., f32::max);
    let mut sorted = correlations.clone();
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let peak = sorted[sorted.len() - 1];
    let mut deviations: Vec<_> = sorted.iter().map(|v| (v - median).abs()).collect();
    deviations.sort_by(f32::total_cmp);
    let mad = deviations[deviations.len() / 2];
    // Dense 8th/16th material makes most lags periodic, so the median and MAD
    // describe music rather than noise. Ask instead whether the clock's lag is
    // (nearly) the strongest periodicity present, and whether that strongest
    // periodicity stands out at all. Over a whole track the autocorrelation's own noise is tiny, so a strong
    // beat lag is significant even when the periodic background inflates the
    // MAD. Overlapping 2048/256 frames leave about N/8 independent samples.
    let noise = (FFT as f32 / HOP as f32 / frames.len() as f32).sqrt();
    let prominence = ((peak - median) / (6. * mad).max(0.025))
        .max((peak - median) / (6. * noise))
        .clamp(0., 1.);
    let dominance = ((supported - median) / (peak - median).max(1e-6) / 0.85).clamp(0., 1.);
    (prominence * dominance).min(alignment_confidence(frames, bpm, phase))
}
