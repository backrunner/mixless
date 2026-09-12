//! Onset autocorrelation and phase refinement.
use super::*;

pub(super) fn tempo(frames: &[Frame], duration: f32) -> (f32, f32, f32) {
    if frames.len() < 128 {
        return (120., 0., 0.);
    }
    let flux: Vec<_> = frames.iter().map(|f| f.onset).collect();
    let dt = HOP as f32 / SR;
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
    // Compute each autocorrelation once, including across phrase probes.
    let correlations: Vec<_> = (min - 1..=max + 1).map(corr).collect();
    let best = (min..=max)
        .max_by(|a, b| correlations[*a - min + 1].total_cmp(&correlations[*b - min + 1]))
        .unwrap_or(min);
    let at = best - min + 1;
    let (left, mid, right) = (correlations[at - 1], correlations[at], correlations[at + 1]);
    let offset = (0.5 * (left - right) / (left - 2. * mid + right).min(-1e-6)).clamp(-0.5, 0.5);
    let mut bpm = 60. / ((best as f32 + offset) * dt);
    // Grid-search phase and tempo across the whole recording; a coarse integer
    // autocorrelation lag alone drifts by an audible fraction of a beat.
    let mut fit = 0.;
    let mut phase = 0.;
    let center = bpm;
    for step in -12..=12 {
        let candidate = center + step as f32 * 0.05;
        let period = 60. / candidate;
        let bins = 64;
        let mut histogram = [0.; 64];
        for f in frames {
            let p = ((f.time / period).fract() * bins as f32) as usize;
            histogram[p.min(63)] += f.onset;
        }
        for j in 0..bins {
            let score = histogram[j] + 0.5 * (histogram[(j + 63) % 64] + histogram[(j + 1) % 64]);
            if score > fit {
                fit = score;
                bpm = candidate;
                phase = (j as f32 + 0.5) / bins as f32 * period;
            }
        }
    }
    let period = 60. / bpm;
    let total = flux.iter().sum::<f32>().max(1e-6);
    let aligned = frames
        .iter()
        .filter(|f| {
            let p = (f.time - phase).rem_euclid(period);
            p.min(period - p) < 0.035
        })
        .map(|f| f.onset)
        .sum::<f32>()
        / total;
    let confidence = (aligned * 1.5).min(mid.max(0.)).clamp(0., 1.) * (duration / 16.).min(1.);
    (bpm.clamp(70., 180.), phase, confidence)
}
