use super::Frame;
use mixless_protocol::MusicalMoment;

pub(super) fn extract(frames: &[Frame], duration: f32) -> Vec<MusicalMoment> {
    let mut result: Vec<MusicalMoment> = Vec::with_capacity((duration * 20.).ceil() as usize);
    let mut onset_scale: Vec<_> = frames.iter().map(|f| f.onset).collect();
    onset_scale.sort_by(f32::total_cmp);
    let scale = onset_scale
        .get(onset_scale.len() * 95 / 100)
        .copied()
        .unwrap_or(1.)
        .max(1e-6);
    let mut low_scale: Vec<_> = frames.iter().map(|f| f.low_onset).collect();
    low_scale.sort_by(f32::total_cmp);
    let low_scale = low_scale
        .get(low_scale.len() * 95 / 100)
        .copied()
        .unwrap_or(1.)
        .max(1e-6);
    for index in 0..(duration * 20.).ceil() as usize {
        let start = index as f32 * 0.05;
        let end = ((index + 1) as f32 * 0.05).min(duration);
        let first = frames.partition_point(|f| f.time < start);
        let last = frames.partition_point(|f| f.time < end);
        let slice = &frames[first..last];
        let count = slice.len().max(1) as f32;
        let rms = (slice.iter().map(|f| f.rms.powi(2)).sum::<f32>() / count).sqrt();
        let bands: [f32; 3] = std::array::from_fn(|i| slice.iter().map(|f| f.band[i]).sum());
        let total = bands.iter().sum::<f32>().max(1e-12);
        let mut chroma =
            std::array::from_fn(|i| slice.iter().map(|f| f.chroma[i]).sum::<f32>() / count);
        let norm = chroma.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        for x in &mut chroma {
            *x /= norm;
        }
        let similarity = result.last().map_or(0., |p| {
            p.chroma
                .iter()
                .zip(chroma)
                .map(|(a, b)| a * b)
                .sum::<f32>()
                .clamp(0., 1.)
        });
        let harmonic = slice.iter().map(|f| f.tonal_risk).sum::<f32>() / count;
        let onset = (slice.iter().map(|f| f.onset).fold(0., f32::max) / scale).clamp(0., 1.);
        let pitch_midi = slice
            .iter()
            .max_by(|a, b| a.tonal_risk.total_cmp(&b.tonal_risk))
            .filter(|f| f.tonal_risk >= 0.18 && f.rms > 0.001)
            .and_then(|f| f.pitch_midi);
        result.push(MusicalMoment {
            start_sec: start,
            end_sec: end,
            rms,
            band_db: bands.map(|x| 10. * (rms * rms * x / total).max(1e-12).log10()),
            attack_sec: slice
                .iter()
                .max_by(|a, b| a.low_onset.total_cmp(&b.low_onset))
                .map_or(start, |f| f.time),
            low_onset: (slice.iter().map(|f| f.low_onset).fold(0., f32::max) / low_scale)
                .clamp(0., 1.),
            chroma,
            onset,
            sustain: (similarity * harmonic * 2.).clamp(0., 1.),
            pitch_midi,
            vocal_confidence: None,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn sustained_tone_and_silence_have_distinct_continuity() {
        let sr = 22050;
        let samples: Vec<f32> = (0..sr * 4)
            .flat_map(|i| {
                let x = if i < sr * 2 {
                    (i as f32 * std::f32::consts::TAU * 440. / sr as f32).sin() * 0.2
                } else {
                    0.
                };
                [x, x]
            })
            .collect();
        let a = super::super::analyze(mixless_protocol::TrackId(0), &samples, sr);
        let active = &a.moments[20];
        assert!(active.sustain > 0.7);
        assert!((active.pitch_midi.unwrap() - 69.).abs() < 0.2);
        assert!(a.moments[60].rms < 0.0001);
        assert_eq!(a.moments[60].pitch_midi, None);
    }
}
