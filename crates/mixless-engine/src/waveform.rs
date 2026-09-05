//! Offline overview-waveform extraction. Runs on the caller thread at load
//! time, never on the audio callback.

use mixless_protocol::Waveform;

use crate::decode::AudioBuffer;
use crate::dsp::Biquad;

/// Band split points (Hz) for the low/mid/high coloring, close to what
/// DJ software uses for its red/green/blue waveform tinting.
const LOW_HZ: f32 = 250.0;
const HIGH_HZ: f32 = 2_500.0;

/// Compute an overview waveform with `columns` columns from a decoded buffer.
///
/// Single pass: each stereo channel runs through a low-pass and a high-pass so
/// each column can report how its energy splits across bands. Peaks are
/// normalized to the track maximum; band shares are per-column ratios.
pub fn compute_waveform(buf: &AudioBuffer, columns: usize) -> Waveform {
    let frames = buf.frames as usize;
    let columns = columns.max(1);
    let duration_sec = frames as f32 / buf.sample_rate.max(1) as f32;
    if frames == 0 {
        return Waveform {
            columns: columns as u32,
            duration_sec,
            peak: vec![0; columns],
            peak_pos: vec![0; columns],
            peak_neg: vec![0; columns],
            rms: vec![0; columns],
            low: vec![0; columns],
            mid: vec![0; columns],
            high: vec![0; columns],
        };
    }

    let sr = buf.sample_rate as f32;
    let mut lp = std::array::from_fn::<_, 2, _>(|_| {
        Biquad::lowpass(sr, LOW_HZ, std::f32::consts::FRAC_1_SQRT_2)
    });
    let mut hp = std::array::from_fn::<_, 2, _>(|_| {
        Biquad::highpass(sr, HIGH_HZ, std::f32::consts::FRAC_1_SQRT_2)
    });

    let per_col = frames as f64 / columns as f64;
    let mut peak_pos = vec![0.0f32; columns];
    let mut peak_neg = vec![0.0f32; columns];
    let mut sum_sq = vec![0.0f32; columns];
    let mut sample_count = vec![0u32; columns];
    let mut e_low = vec![0.0f32; columns];
    let mut e_mid = vec![0.0f32; columns];
    let mut e_high = vec![0.0f32; columns];

    for i in 0..frames {
        let c = ((i as f64 / per_col) as usize).min(columns - 1);
        for ch in 0..2 {
            let x = buf.samples[i * 2 + ch];
            let low = lp[ch].process(x);
            let high = hp[ch].process(x);
            let mid = x - low - high;
            peak_pos[c] = peak_pos[c].max(x.max(0.0));
            peak_neg[c] = peak_neg[c].max((-x).max(0.0));
            sum_sq[c] += x * x;
            sample_count[c] += 1;
            e_low[c] += low * low;
            e_mid[c] += mid * mid;
            e_high[c] += high * high;
        }
    }

    let max_peak = peak_pos
        .iter()
        .chain(peak_neg.iter())
        .copied()
        .fold(0.0f32, f32::max)
        .max(1e-6);
    let mut out = Waveform {
        columns: columns as u32,
        duration_sec,
        peak: Vec::with_capacity(columns),
        peak_pos: Vec::with_capacity(columns),
        peak_neg: Vec::with_capacity(columns),
        rms: Vec::with_capacity(columns),
        low: Vec::with_capacity(columns),
        mid: Vec::with_capacity(columns),
        high: Vec::with_capacity(columns),
    };
    for c in 0..columns {
        let pos = (peak_pos[c] / max_peak * 255.0).round() as u8;
        let neg = (peak_neg[c] / max_peak * 255.0).round() as u8;
        out.peak_pos.push(pos);
        out.peak_neg.push(neg);
        out.peak.push(pos.max(neg));
        let rms = if sample_count[c] > 0 {
            (sum_sq[c] / sample_count[c] as f32).sqrt()
        } else {
            0.0
        };
        out.rms
            .push((rms / max_peak * 255.0).clamp(0.0, 255.0).round() as u8);
        let total = e_low[c] + e_mid[c] + e_high[c];
        if total > 1e-9 {
            out.low.push((e_low[c] / total * 255.0).round() as u8);
            out.mid.push((e_mid[c] / total * 255.0).round() as u8);
            out.high.push((e_high[c] / total * 255.0).round() as u8);
        } else {
            // Silent column: neutral gray tint.
            out.low.push(85);
            out.mid.push(85);
            out.high.push(85);
        }
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
            samples,
            frames: frames as u64,
            sample_rate: sr,
        })
    }

    #[test]
    fn antiphase_stereo_is_not_silence() {
        let original = tone(48_000, 80.0, 0.5);
        let mut inverted = AudioBuffer {
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
