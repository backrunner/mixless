//! High-resolution pitch-class evidence, separate from transient analysis.
//! Spectral peaks are interpolated in log magnitude; local whitening and
//! per-frame normalization keep a loud bass note or mastered drop from
//! dominating the whole song. Tuning is measured before semitone folding.
use super::{key, Complex, FftPlanner, SR};

const SIZE: usize = 8192;
const STEP: usize = 4096;

pub(super) struct TonalAnalysis {
    pub key: (Option<String>, Option<String>, f32),
    pub frames: Vec<(f32, [f32; 12])>,
}

impl TonalAnalysis {
    pub fn into_frames(self, duration: f32) -> Vec<mixless_protocol::TonalFrame> {
        self.frames
            .into_iter()
            .map(|(time, chroma)| mixless_protocol::TonalFrame {
                start_sec: (time - STEP as f32 / (2. * SR)).max(0.),
                end_sec: (time + STEP as f32 / (2. * SR)).min(duration),
                chroma,
            })
            .collect()
    }
    pub fn chroma(&self, start: f32, end: f32) -> [f32; 12] {
        let mut chroma = [0.; 12];
        let first = self.frames.partition_point(|f| f.0 < start);
        let last = self.frames.partition_point(|f| f.0 < end);
        for (_, frame) in &self.frames[first..last] {
            for i in 0..12 {
                chroma[i] += frame[i] / (last - first) as f32;
            }
        }
        chroma
    }
}

pub(super) fn analyze(mono: &[f32]) -> TonalAnalysis {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(SIZE);
    let window: Vec<_> = (0..SIZE)
        .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / SIZE as f32).cos())
        .collect();
    let mut bins = vec![Complex::new(0., 0.); SIZE];
    let mut scratch = vec![Complex::new(0., 0.); fft.get_inplace_scratch_len()];
    let mut magnitudes = vec![0f32; SIZE / 2];
    let mut frames = Vec::new();
    let mut tuning = [0f32; 2];
    for start in (0..mono.len().saturating_sub(SIZE)).step_by(STEP) {
        for n in 0..SIZE {
            bins[n] = Complex::new(mono[start + n] * window[n], 0.);
        }
        fft.process_with_scratch(&mut bins, &mut scratch);
        for (mag, bin) in magnitudes.iter_mut().zip(&bins) {
            *mag = bin.norm();
        }
        let max = magnitudes.iter().copied().fold(0., f32::max);
        if max < 1e-5 {
            continue;
        }
        let mut peaks = Vec::new();
        for k in 16..(3500. * SIZE as f32 / SR) as usize {
            let mag = magnitudes[k];
            if mag < max * 0.008 || mag <= magnitudes[k - 1] || mag <= magnitudes[k + 1] {
                continue;
            }
            let (a, b, c) = (
                magnitudes[k - 1].max(1e-12).ln(),
                mag.ln(),
                magnitudes[k + 1].max(1e-12).ln(),
            );
            let delta = (0.5 * (a - c) / (a - 2. * b + c).min(-1e-8)).clamp(-0.5, 0.5);
            let hz = (k as f32 + delta) * SR / SIZE as f32;
            let note = 69. + 12. * (hz / 440.).log2();
            // Broad local spectral envelope, excluding the peak's main lobe.
            let radius = (k / 8).max(8);
            let mut envelope = 0.;
            let mut count = 0;
            for i in k.saturating_sub(radius)..(k + radius).min(magnitudes.len()) {
                if i.abs_diff(k) > 2 {
                    envelope += magnitudes[i];
                    count += 1;
                }
            }
            let prominence = (mag / (envelope / count.max(1) as f32).max(max * 0.003)).min(12.);
            if prominence < 2. {
                continue;
            }
            let weight = (mag / max).sqrt() * prominence.sqrt();
            peaks.push((note, weight));
        }
        peaks.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
        peaks.truncate(60);
        let sum = peaks.iter().map(|p| p.1).sum::<f32>();
        if sum < 1e-6 {
            continue;
        }
        for (note, weight) in &mut peaks {
            *weight /= sum;
            let angle = note.fract() * std::f32::consts::TAU;
            tuning[0] += angle.cos() * *weight;
            tuning[1] += angle.sin() * *weight;
        }
        frames.push(((start + SIZE / 2) as f32 / SR, peaks));
    }
    let detuning = tuning[1].atan2(tuning[0]) / std::f32::consts::TAU;
    let mut global = [0.; 12];
    let mut windows = Vec::new();
    let mut local = [0.; 12];
    let mut window_end = 16.;
    let mut chroma_frames = Vec::with_capacity(frames.len());
    for (time, peaks) in &frames {
        // Skipped silent frames must not make a sixteen-second context span
        // unrelated material on the other side of a long pause.
        if *time >= window_end {
            if local.iter().sum::<f32>() > 0. {
                windows.push(key(&local));
            }
            local = [0.; 12];
            window_end = ((*time / 16.).floor() + 1.) * 16.;
        }
        let mut chroma = [0.; 12];
        for &(note, weight) in peaks {
            let pitch = note - detuning;
            let center = pitch.round();
            let distance = pitch - center;
            let cosine = (std::f32::consts::PI * distance).cos().max(0.);
            chroma[(center as i32).rem_euclid(12) as usize] += weight * cosine * cosine;
        }
        let sum = chroma.iter().sum::<f32>().max(1e-8);
        for pc in 0..12 {
            chroma[pc] /= sum;
            global[pc] += chroma[pc];
            local[pc] += chroma[pc];
        }
        chroma_frames.push((*time, chroma));
    }
    if local.iter().sum::<f32>() > 0. {
        windows.push(key(&local));
    }
    let (key, camelot, confidence) = key(&global);
    // Modulations and relative-major/minor ambiguity lower confidence. They
    // never acquire a high-confidence label just by containing more frames.
    let agreement =
        windows.iter().filter(|w| w.0 == key).count() as f32 / windows.len().max(1) as f32;
    TonalAnalysis {
        key: (key, camelot, confidence * (0.4 + 0.6 * agreement)),
        frames: chroma_frames,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_spectra_preserve_two_keys_across_silence_time_shift_and_gain() {
        let tone = |root: f32| {
            (0..(SR * 4.) as usize)
                .map(|i| {
                    let t = i as f32 / SR;
                    [0., 4., 7.]
                        .iter()
                        .map(|n| {
                            let hz = 440. * 2f32.powf((48. + root + n - 69.) / 12.);
                            (std::f32::consts::TAU * hz * t).sin()
                        })
                        .sum::<f32>()
                        * 0.2
                })
                .collect::<Vec<_>>()
        };
        for gain in [0.01, 1., 2.] {
            for offset in [0., 2.] {
                let mut samples = vec![0.; (offset * SR) as usize];
                samples.extend(tone(0.).into_iter().map(|v| v * gain));
                samples.extend(vec![0.; (20. * SR) as usize]);
                samples.extend(tone(6.).into_iter().map(|v| v * gain));
                let result = analyze(&samples);
                assert_eq!(
                    key(&result.chroma(offset + 0.5, offset + 3.5)).1.as_deref(),
                    Some("8B")
                );
                assert_eq!(
                    key(&result.chroma(offset + 24.5, offset + 27.5))
                        .1
                        .as_deref(),
                    Some("2B")
                );
                assert_eq!(
                    key(&result.chroma(offset + 5., offset + 23.)),
                    (None, None, 0.)
                );
            }
        }
    }
    #[test]
    fn all_major_and_minor_triads_survive_detuning_and_unequal_harmonics() {
        for root in 0..12 {
            for (mode, third) in [("major", 4), ("minor", 3)] {
                let mut samples = vec![0.; (SR * 3.) as usize];
                for (i, sample) in samples.iter_mut().enumerate() {
                    let t = i as f32 / SR;
                    for (interval, level) in [(0, 1.), (third, 0.7), (7, 0.65)] {
                        let hz =
                            440. * 2f32.powf((48. + (root + interval) as f32 + 0.28 - 69.) / 12.);
                        *sample += level
                            * ((std::f32::consts::TAU * hz * t).sin()
                                + 0.25 * (std::f32::consts::TAU * hz * 2. * t).sin());
                    }
                }
                let result = analyze(&samples);
                let names = [
                    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
                ];
                assert_eq!(
                    result.key.0,
                    Some(format!("{} {mode}", names[root])),
                    "root {root}, {mode}"
                );
            }
        }
    }
}
