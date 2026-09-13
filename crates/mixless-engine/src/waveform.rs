//! Offline high-resolution envelopes and time-smoothed four-band energy.
use crate::{decode::AudioBuffer, dsp::Biquad};
use mixless_protocol::Waveform;

/// Fast, unsmoothed amplitude-only placeholder. Never persisted as spectral
/// analysis: the host replaces it with `compute_waveform` when ready.
pub fn compute_preview_waveform(buf: &AudioBuffer, columns: usize) -> Waveform {
    let frames = buf.frames as usize;
    let columns = columns.clamp(1, frames.max(1));
    let mut peak = Vec::with_capacity(columns);
    for c in 0..columns {
        let start = c * frames / columns * 2;
        let end = (c + 1) * frames / columns * 2;
        peak.push(
            buf.samples[start..end]
                .iter()
                .map(|x| x.abs())
                .fold(0., f32::max),
        );
    }
    let maximum = peak.iter().copied().fold(1e-6, f32::max);
    let detail: Vec<_> = peak
        .iter()
        .map(|x| (x / maximum * 65535.).round() as u16)
        .collect();
    Waveform {
        columns: columns as u32,
        duration_sec: frames as f32 / buf.sample_rate.max(1) as f32,
        peak: peak
            .iter()
            .map(|x| (x / maximum * 255.).round() as u8)
            .collect(),
        peak_pos: vec![],
        peak_neg: vec![],
        rms: vec![],
        low: vec![0; columns],
        low_mid: vec![0; columns],
        mid: vec![255; columns],
        high: vec![0; columns],
        detail_pos: detail.clone(),
        detail_neg: detail,
        detail_rms: vec![],
    }
}

/// Independent band filters avoid the phase cancellation of `x - low - high`.
/// Only spectral color is smoothed; attack locations retain sample-bin accuracy.
pub fn compute_waveform(buf: &AudioBuffer, columns: usize) -> Waveform {
    let frames = buf.frames as usize;
    let columns = columns.max(1).min(frames.max(1));
    let sr = buf.sample_rate.max(1) as f32;
    let duration_sec = frames as f32 / sr;
    let q = std::f32::consts::FRAC_1_SQRT_2;
    let mut low = std::array::from_fn::<_, 2, _>(|_| Biquad::lowpass(sr, 180., q));
    let mut low_mid = std::array::from_fn::<_, 2, _>(|_| {
        [Biquad::highpass(sr, 180., q), Biquad::lowpass(sr, 800., q)]
    });
    let mut mid = std::array::from_fn::<_, 2, _>(|_| {
        [Biquad::highpass(sr, 800., q), Biquad::lowpass(sr, 4000., q)]
    });
    let mut high = std::array::from_fn::<_, 2, _>(|_| Biquad::highpass(sr, 4000., q));
    let mut envelope = vec![[0f32; 3]; columns];
    let mut energy = vec![[0f64; 4]; columns];
    for c in 0..columns {
        let first = (c * frames).div_ceil(columns);
        let last = ((c + 1) * frames).div_ceil(columns);
        for frame in buf.samples[first * 2..last * 2].chunks_exact(2) {
            for ch in 0..2 {
                let x = frame[ch];
                envelope[c][0] = envelope[c][0].max(x.max(0.));
                envelope[c][1] = envelope[c][1].max((-x).max(0.));
                envelope[c][2] += x * x;
                let lm = low_mid[ch][0].process(x);
                let m = mid[ch][0].process(x);
                let bands = [
                    low[ch].process(x),
                    low_mid[ch][1].process(lm),
                    mid[ch][1].process(m),
                    high[ch].process(x),
                ];
                for band in 0..4 {
                    energy[c][band] += bands[band] as f64 * bands[band] as f64;
                }
            }
        }
        envelope[c][2] = (envelope[c][2] / ((last - first) * 2).max(1) as f32).sqrt();
    }
    // Centered 16 ms energy window: a sustained bass note keeps one color as
    // its waveform crosses zero. Prefix sums keep extraction linear in length.
    let mut sums = vec![[0f64; 4]; columns + 1];
    for i in 0..columns {
        for band in 0..4 {
            sums[i + 1][band] = sums[i][band] + energy[i][band];
        }
    }
    let radius = (0.008 * columns as f32 / duration_sec.max(0.001)).ceil() as usize;
    let max_peak = envelope
        .iter()
        .flat_map(|p| [p[0], p[1]])
        .fold(1e-6f32, f32::max);
    let mut out = Waveform {
        columns: columns as u32,
        duration_sec,
        peak: Vec::with_capacity(columns),
        peak_pos: Vec::with_capacity(columns),
        peak_neg: Vec::with_capacity(columns),
        rms: Vec::with_capacity(columns),
        low: Vec::with_capacity(columns),
        low_mid: Vec::with_capacity(columns),
        mid: Vec::with_capacity(columns),
        high: Vec::with_capacity(columns),
        detail_pos: Vec::with_capacity(columns),
        detail_neg: Vec::with_capacity(columns),
        detail_rms: Vec::with_capacity(columns),
    };
    for c in 0..columns {
        let detail = envelope[c].map(|v| (v / max_peak * 65535.).clamp(0., 65535.).round() as u16);
        let coarse = detail.map(|v| ((v as u32 + 128) / 257) as u8);
        out.peak.push(coarse[0].max(coarse[1]));
        out.peak_pos.push(coarse[0]);
        out.peak_neg.push(coarse[1]);
        out.rms.push(coarse[2]);
        out.detail_pos.push(detail[0]);
        out.detail_neg.push(detail[1]);
        out.detail_rms.push(detail[2]);
        let left = c.saturating_sub(radius);
        let right = (c + radius + 1).min(columns);
        let e: [f64; 4] = std::array::from_fn(|b| (sums[right][b] - sums[left][b]).max(0.));
        let total = e.iter().sum::<f64>().max(1e-20);
        let bands = e.map(|v| (v / total * 255.).round() as u8);
        out.low.push(bands[0]);
        out.low_mid.push(bands[1]);
        out.mid.push(bands[2]);
        out.high.push(bands[3]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn tone(sr: u32, hz: f32, secs: f32) -> Arc<AudioBuffer> {
        let frames = (sr as f32 * secs) as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for n in 0..frames {
            let s = (2.0 * std::f32::consts::PI * hz * n as f32 / sr as f32).sin() * 0.5;
            samples.push(s);
            samples.push(s);
        }
        Arc::new(AudioBuffer {
            loudness: Default::default(),
            samples,
            frames: frames as u64,
            sample_rate: sr,
        })
    }

    #[test]
    fn mid_bands_are_separate_and_detail_keeps_more_than_eight_bits() {
        let lm = compute_waveform(&tone(48000, 400., 0.2), 300);
        let hm = compute_waveform(&tone(48000, 1800., 0.2), 300);
        assert!(lm.low_mid[150] > lm.low[150] && lm.low_mid[150] > lm.mid[150]);
        assert!(hm.mid[150] > hm.low_mid[150] && hm.mid[150] > hm.high[150]);
        let buf = AudioBuffer {
            loudness: Default::default(),
            samples: (0..1024).flat_map(|i| [i as f32 / 2048.; 2]).collect(),
            frames: 1024,
            sample_rate: 48000,
        };
        let ramp = compute_waveform(&buf, 1024);
        assert!(ramp.detail_pos.windows(2).all(|v| v[1] > v[0]));
        assert!(ramp.peak_pos.windows(2).any(|v| v[1] == v[0]));
    }

    #[test]
    fn short_audio_has_no_empty_columns_before_its_first_sample() {
        let buf = AudioBuffer {
            loudness: Default::default(),
            samples: vec![0.25; 16],
            frames: 8,
            sample_rate: 48_000,
        };
        let wave = compute_waveform(&buf, 4096);
        assert_eq!(wave.columns, 8);
        assert!(wave.peak.iter().all(|p| *p == 255));
    }

    #[test]
    fn antiphase_stereo_is_not_silence() {
        let original = tone(48_000, 80.0, 0.5);
        let mut inverted = AudioBuffer {
            loudness: Default::default(),
            samples: original.samples.clone(),
            frames: original.frames,
            sample_rate: original.sample_rate,
        };
        for sample in inverted.samples.iter_mut().skip(1).step_by(2) {
            *sample = -*sample;
        }
        let same = compute_waveform(&original, 64);
        let opposite = compute_waveform(&inverted, 64);
        assert_eq!(same.peak, opposite.peak);
        assert_eq!(same.rms, opposite.rms);
        assert_eq!(same.low, opposite.low);
        assert!(opposite.peak[32] > 200);
    }

    #[test]
    fn low_tone_colors_low_band() {
        let w = compute_waveform(&tone(48_000, 80.0, 0.5), 64);
        assert_eq!(w.peak.len(), 64);
        // Skip the first columns while the filters settle.
        let c = 32;
        assert!(
            w.low[c] > 150,
            "low={} mid={} high={}",
            w.low[c],
            w.mid[c],
            w.high[c]
        );
        assert!(w.low[c] > w.mid[c] * 2, "low={} mid={}", w.low[c], w.mid[c]);
        assert!(w.peak[c] > 200);
    }

    #[test]
    fn high_tone_colors_high_band() {
        let w = compute_waveform(&tone(48_000, 8_000.0, 0.5), 64);
        let c = 32;
        assert!(
            w.high[c] > 200,
            "low={} mid={} high={}",
            w.low[c],
            w.mid[c],
            w.high[c]
        );
    }

    #[test]
    fn silence_is_zero_peak() {
        let frames = 24_000usize;
        let buf = AudioBuffer {
            loudness: Default::default(),
            samples: vec![0.0; frames * 2],
            frames: frames as u64,
            sample_rate: 48_000,
        };
        let w = compute_waveform(&buf, 32);
        assert!(w.peak.iter().all(|&p| p == 0));
        assert!(w.peak_pos.iter().all(|&p| p == 0));
        assert!(w.peak_neg.iter().all(|&p| p == 0));
        assert!(w.rms.iter().all(|&p| p == 0));
    }

    #[test]
    fn positive_signal_keeps_asymmetric_peaks() {
        let frames = 4_800usize;
        let buf = AudioBuffer {
            loudness: Default::default(),
            samples: vec![0.25; frames * 2],
            frames: frames as u64,
            sample_rate: 48_000,
        };
        let w = compute_waveform(&buf, 32);
        assert!(w.peak_pos.iter().all(|&p| p == 255));
        assert!(w.peak_neg.iter().all(|&p| p == 0));
        assert!(w.rms.iter().all(|&p| p == 255));
    }
}
