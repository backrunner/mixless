//! Shared chroma-profile estimator for whole-track and exact overlap evidence.
/// Transpose a Camelot key by an integral number of semitones. A fractional
/// detuning is not a different equal-tempered key.
pub fn transpose_camelot(key: &str, semitones: f32) -> Option<String> {
    if !semitones.is_finite() || (semitones - semitones.round()).abs() > 0.05 {
        return None;
    }
    let (number, mode) = key.split_at_checked(key.len().checked_sub(1)?)?;
    let table = match mode {
        "A" => [8, 3, 10, 5, 0, 7, 2, 9, 4, 11, 6, 1],
        "B" => [11, 6, 1, 8, 3, 10, 5, 0, 7, 2, 9, 4],
        _ => return None,
    };
    let index = number.parse::<usize>().ok()?.checked_sub(1)?;
    let pc = (*table.get(index)? + (semitones.round().rem_euclid(12.)) as i32) % 12;
    Some(format!(
        "{}{}",
        table.iter().position(|v| *v == pc)? + 1,
        mode
    ))
}

pub fn estimate_key(chroma: &[f32; 12]) -> (Option<String>, Option<String>, f32) {
    estimate(chroma, false)
}

/// Relative major/minor use the same pitch classes. Their ambiguity must not
/// make an otherwise clear harmonic match uncertain. Other competing keys
/// still reduce confidence, including adjacent keys on the circle of fifths.
pub fn estimate_harmonic_key(chroma: &[f32; 12]) -> (Option<String>, Option<String>, f32) {
    estimate(chroma, true)
}

fn estimate(chroma: &[f32; 12], harmonic: bool) -> (Option<String>, Option<String>, f32) {
    let sum = chroma.iter().sum::<f32>();
    if sum < 1e-8 {
        return (None, None, 0.);
    }
    let major = [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ];
    let minor = [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ];
    let names = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let cams = [
        [
            "8B", "3B", "10B", "5B", "12B", "7B", "2B", "9B", "4B", "11B", "6B", "1B",
        ],
        [
            "5A", "12A", "7A", "2A", "9A", "4A", "11A", "6A", "1A", "8A", "3A", "10A",
        ],
    ];
    let mut scores = Vec::new();
    let mean = sum / 12.;
    for (mode, template) in [major, minor].iter().enumerate() {
        let tm = template.iter().sum::<f32>() / 12.;
        for root in 0..12 {
            let mut dot = 0.;
            let mut aa = 0.;
            let mut bb = 0.;
            for i in 0..12 {
                let a = chroma[i] - mean;
                let b = template[(i + 12 - root) % 12] - tm;
                dot += a * b;
                aa += a * a;
                bb += b * b;
            }
            scores.push((dot / (aa * bb).sqrt().max(1e-8), mode, root));
        }
    }
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (score, mode, root) = scores[0];
    let family = |mode: usize, root: usize| if mode == 0 { root } else { (root + 3) % 12 };
    let runner = scores
        .iter()
        .skip(1)
        .find(|(_, m, r)| !harmonic || family(*m, *r) != family(mode, root))
        .unwrap()
        .0;
    let confidence = (score.max(0.) * ((score - runner).max(0.) * 6.).min(1.)).clamp(0., 1.);
    (
        Some(format!(
            "{} {}",
            names[root],
            if mode == 0 { "major" } else { "minor" }
        )),
        Some(cams[mode][root].into()),
        confidence,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transposition_wraps_pitch_classes_and_rejects_unsupported_keys() {
        assert_eq!(transpose_camelot("7B", -3.).as_deref(), Some("10B"));
        assert_eq!(transpose_camelot("10A", 12.).as_deref(), Some("10A"));
        for mode in ["A", "B"] {
            for number in 1..=12 {
                let key = format!("{number}{mode}");
                for pitch in -12..=12 {
                    let shifted = transpose_camelot(&key, pitch as f32).unwrap();
                    assert_eq!(
                        transpose_camelot(&shifted, -pitch as f32),
                        Some(key.clone())
                    );
                }
            }
        }
        for key in ["", "0A", "13B", "8C", "音"] {
            assert!(transpose_camelot(key, 0.).is_none());
        }
        assert!(transpose_camelot("8A", f32::NAN).is_none());
        assert!(transpose_camelot("8A", 0.5).is_none());
    }

    #[test]
    fn relative_mode_ambiguity_does_not_hide_clear_tonal_evidence() {
        // C major and A minor share the same diatonic notes.
        let chroma = [1., 0., 0.5, 0., 0.8, 0.4, 0., 0.7, 0., 1., 0., 0.4];
        let (_, key, confidence) = estimate_key(&chroma);
        let (_, harmonic, certainty) = estimate_harmonic_key(&chroma);
        assert_eq!(key, harmonic);
        assert!(certainty >= confidence);
        assert!(certainty > 0.55, "{certainty}");
        assert_eq!(estimate_harmonic_key(&[0.; 12]).2, 0.);
        assert_eq!(estimate_harmonic_key(&[1.; 12]).2, 0.);
    }
}
