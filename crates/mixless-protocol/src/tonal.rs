//! Shared chroma-profile estimator for whole-track and exact overlap evidence.
pub fn estimate_key(chroma: &[f32; 12]) -> (Option<String>, Option<String>, f32) {
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
    let confidence = (score.max(0.) * ((score - scores[1].0).max(0.) * 6.).min(1.)).clamp(0., 1.);
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
