//! Offline band-limited conversion; preserves sample-zero timing and stereo.
pub fn convert(input: &[f32], channels: usize, from: u32, to: u32) -> Vec<f32> {
    if from == to {
        return input.to_vec();
    }
    let frames = input.len() / channels;
    let n = (frames as u64 * to as u64).div_ceil(from as u64) as usize;
    let ratio = from as f64 / to as f64;
    let cutoff = 0.94 / ratio.max(1.);
    const RADIUS: i64 = 32;
    const PHASES: usize = 1024;
    let kernels: Vec<Vec<f32>> = (0..PHASES)
        .map(|phase| {
            let fraction = phase as f64 / PHASES as f64;
            let mut weights: Vec<f32> = (-RADIUS + 1..=RADIUS)
                .map(|tap| {
                    let d = tap as f64 - fraction;
                    let x = std::f64::consts::PI * d * cutoff;
                    let sinc = if x.abs() < 1e-10 {
                        cutoff
                    } else {
                        x.sin() / (std::f64::consts::PI * d)
                    };
                    let w = 0.5 + 0.5 * (std::f64::consts::PI * d / RADIUS as f64).cos();
                    (sinc * w) as f32
                })
                .collect();
            let sum: f32 = weights.iter().sum();
            weights.iter_mut().for_each(|w| *w /= sum);
            weights
        })
        .collect();
    let mut out = vec![0.; n * channels];
    if frames == 0 {
        return out;
    }
    for f in 0..n {
        let at = f as f64 * ratio;
        let center = at.floor() as i64;
        let weights = &kernels[((at.fract() * PHASES as f64) as usize).min(PHASES - 1)];
        for (tap, weight) in (-RADIUS + 1..=RADIUS).zip(weights) {
            let source = (center + tap).clamp(0, frames as i64 - 1) as usize;
            for c in 0..channels {
                out[f * channels + c] += input[source * channels + c] * weight;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_preserves_alignment_and_rejects_aliasing() {
        let signal: Vec<_> = (0..48000)
            .flat_map(|i| {
                [
                    0.25,
                    (std::f32::consts::TAU * 18000. * i as f32 / 48000.).sin(),
                ]
            })
            .collect();
        let out = convert(&signal, 2, 48000, 22050);
        assert_eq!(out.len(), 44100);
        assert!(out.chunks_exact(2).all(|s| (s[0] - 0.25).abs() < 1e-5));
        assert!(
            out.chunks_exact(2)
                .skip(50)
                .take(21000)
                .map(|s| s[1] * s[1])
                .sum::<f32>()
                / 21000.
                < 0.0001
        );
    }
}
