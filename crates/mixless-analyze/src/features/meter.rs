//! Simple-meter evidence from recurring low-frequency accents on the measured
//! pulse. A strong pulse alone cannot establish either meter or bar one.

pub(super) struct Meter {
    pub beats: usize,
    pub phase: usize,
    pub confidence: f32,
}

fn candidate(accents: &[f32], beats: usize) -> Meter {
    let mut sum = vec![0.; beats];
    let mut count = vec![0; beats];
    for (i, &value) in accents.iter().enumerate() {
        sum[i % beats] += value;
        count[i % beats] += 1;
    }
    // Equal support per phase: a clipped final bar must not manufacture a
    // stronger bar one simply because its phase has one extra observation.
    for (sum, count) in sum.iter_mut().zip(&count) {
        *sum /= (*count).max(1) as f32;
    }
    let phase = (0..beats)
        .max_by(|a, b| sum[*a].total_cmp(&sum[*b]))
        .unwrap();
    let runner_up = sum
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != phase)
        .map(|(_, v)| *v)
        .fold(0., f32::max);
    let contrast = ((sum[phase] - runner_up) / sum[phase].max(1e-6) / 0.3).clamp(0., 1.);
    // Correlation at the proposed bar period distinguishes a repeated accent
    // from an isolated impact or unrelated accents collected into one bucket.
    let pairs = accents.len().saturating_sub(beats);
    let repeat = if pairs < beats * 3 {
        0.
    } else {
        let a = &accents[..pairs];
        let b = &accents[beats..];
        let mean_a = a.iter().sum::<f32>() / pairs as f32;
        let mean_b = b.iter().sum::<f32>() / pairs as f32;
        let (mut covariance, mut va, mut vb) = (0., 0., 0.);
        for (&a, &b) in a.iter().zip(b) {
            let (a, b) = (a - mean_a, b - mean_b);
            covariance += a * b;
            va += a * a;
            vb += b * b;
        }
        if va * vb <= 1e-12 {
            0.
        } else {
            (covariance / (va * vb).sqrt()).clamp(0., 1.)
        }
    };
    Meter {
        beats,
        phase,
        confidence: contrast.min(repeat),
    }
}

pub(super) fn detect(accents: &[f32]) -> Meter {
    let mut candidates: Vec<_> = [4, 3, 5, 7]
        .into_iter()
        .map(|m| candidate(accents, m))
        .collect();
    candidates.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    let best = &candidates[0];
    let separation =
        ((best.confidence - candidates[1].confidence) / best.confidence.max(1e-6) / 0.3)
            .clamp(0., 1.);
    let mut confidence = best.confidence.min(separation);
    // A different simple meter needs agreement in both halves, with phase
    // expressed relative to the same first beat. Changing-meter recordings
    // cannot inherit a confident whole-track odd meter from one passage.
    if best.beats != 4 {
        let middle = accents.len() / 2;
        let left = candidate(&accents[..middle], best.beats);
        let right = candidate(&accents[middle..], best.beats);
        confidence = confidence.min(left.confidence).min(right.confidence);
        if left.phase != best.phase || (right.phase + middle) % best.beats != best.phase {
            confidence = 0.;
        }
    }
    if confidence >= 0.65 {
        Meter {
            beats: best.beats,
            phase: best.phase,
            confidence,
        }
    } else {
        // Preserve a four-pulse timing hypothesis for uncertain/legacy
        // consumers, while withholding a claim of reliable bar-one evidence.
        let mut fallback = candidate(accents, 4);
        fallback.confidence = if best.beats == 4 { confidence } else { 0. };
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_simple_meters_survive_gain_pickup_and_incomplete_last_bars() {
        for meter in [3, 4, 5, 7] {
            for pickup in 0..meter {
                for gain in [0.1, 1., 3.] {
                    let accents: Vec<_> = (0..meter * 24 + 2)
                        .map(|i| gain * if (i + pickup) % meter == 0 { 1. } else { 0.4 })
                        .collect();
                    let found = detect(&accents);
                    assert_eq!(found.beats, meter);
                    assert_eq!((found.phase + pickup) % meter, 0);
                    assert!(found.confidence >= 0.65);
                }
            }
        }
    }

    #[test]
    fn uniform_alternating_and_isolated_impacts_do_not_prove_bar_one() {
        for accents in [
            vec![1.; 120],
            (0..120)
                .map(|i| if i % 2 == 0 { 1. } else { 0.4 })
                .collect(),
            (0..120).map(|i| if i == 31 { 1. } else { 0.1 }).collect(),
            vec![0.; 120],
        ] {
            assert!(detect(&accents).confidence < 0.65);
        }
    }

    #[test]
    fn changing_meter_cannot_claim_a_global_odd_meter() {
        let accents: Vec<_> = (0..240)
            .map(|i| {
                let meter = if i < 120 { 3 } else { 4 };
                if i % meter == 0 {
                    1.
                } else {
                    0.4
                }
            })
            .collect();
        assert!(detect(&accents).confidence < 0.65);
    }
}
