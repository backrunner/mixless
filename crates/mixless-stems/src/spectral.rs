//! Native host STFT for the pinned BS-RoFormer graph. No per-stem normalization.
use crate::{check, Error, Result};
use rustfft::{num_complex::Complex32, FftPlanner};

pub(crate) const FRAMES: usize = 352800;
pub(crate) const FFT: usize = 2048;
pub(crate) const HOP: usize = 441;
pub(crate) const BINS: usize = FFT / 2 + 1;
pub(crate) const STEPS: usize = FRAMES / HOP + 1;
pub(crate) const SPECTRUM: usize = 2 * BINS * STEPS;

pub(crate) fn reflect(index: isize, len: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    let period = 2 * (len as isize - 1);
    let at = index.rem_euclid(period) as usize;
    if at < len {
        at
    } else {
        period as usize - at
    }
}
fn window(i: usize) -> f32 {
    0.5 - 0.5 * (2. * std::f32::consts::PI * i as f32 / FFT as f32).cos()
}

pub(crate) fn analyze(audio: &[f32], active: &impl Fn() -> bool) -> Result<(Vec<f32>, Vec<f32>)> {
    if audio.len() != FRAMES * 2 || audio.iter().any(|s| !s.is_finite()) {
        return Err(Error::Model("Invalid BS-RoFormer input PCM".into()));
    }
    let fft = FftPlanner::new().plan_fft_forward(FFT);
    let mut block = vec![Complex32::default(); FFT];
    let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
    let mut real = vec![0.; SPECTRUM];
    let mut imag = vec![0.; SPECTRUM];
    let hann: Vec<_> = (0..FFT).map(window).collect();
    for channel in 0..2 {
        for frame in 0..STEPS {
            check(active)?;
            for i in 0..FFT {
                let at = reflect((frame * HOP + i) as isize - (FFT / 2) as isize, FRAMES);
                block[i] = Complex32::new(audio[at * 2 + channel] * hann[i], 0.);
            }
            fft.process_with_scratch(&mut block, &mut scratch);
            for bin in 0..BINS {
                let at = (channel * BINS + bin) * STEPS + frame;
                real[at] = block[bin].re;
                imag[at] = block[bin].im;
            }
        }
    }
    Ok((real, imag))
}

pub(crate) fn synthesize(
    real: &[f32],
    imag: &[f32],
    active: &impl Fn() -> bool,
) -> Result<Vec<f32>> {
    if real.len() != SPECTRUM
        || imag.len() != SPECTRUM
        || real.iter().chain(imag).any(|s| !s.is_finite())
    {
        return Err(Error::Model("Invalid BS-RoFormer spectrum".into()));
    }
    let fft = FftPlanner::new().plan_fft_inverse(FFT);
    let mut block = vec![Complex32::default(); FFT];
    let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
    let padded = (STEPS - 1) * HOP + FFT;
    let mut output = vec![0.; padded * 2];
    let mut counter = vec![0.; padded];
    let hann: Vec<_> = (0..FFT).map(window).collect();
    for frame in 0..STEPS {
        check(active)?;
        for i in 0..FFT {
            counter[frame * HOP + i] += hann[i] * hann[i];
        }
        for channel in 0..2 {
            for bin in 0..BINS {
                let at = (channel * BINS + bin) * STEPS + frame;
                block[bin] = Complex32::new(real[at], imag[at]);
            }
            for bin in 1..FFT / 2 {
                block[FFT - bin] = block[bin].conj();
            }
            fft.process_with_scratch(&mut block, &mut scratch);
            for i in 0..FFT {
                output[(frame * HOP + i) * 2 + channel] += block[i].re / FFT as f32 * hann[i];
            }
        }
    }
    let mut result = vec![0.; FRAMES * 2];
    for frame in 0..FRAMES {
        let at = frame + FFT / 2;
        if counter[at] <= 1e-8 {
            return Err(Error::Model(
                "Uncovered BS-RoFormer synthesis sample".into(),
            ));
        }
        for channel in 0..2 {
            result[frame * 2 + channel] = output[at * 2 + channel] / counter[at];
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires torch STFT fixtures at MIXLESS_SPECTRAL_REFERENCE_DIR"]
    fn native_stft_matches_torch_centered_hann_reference() {
        let root = std::path::PathBuf::from(
            std::env::var_os("MIXLESS_SPECTRAL_REFERENCE_DIR").expect("reference directory"),
        );
        let read = |name: &str| {
            std::fs::read(root.join(name))
                .unwrap()
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>()
        };
        let pcm = read("input.f32");
        let expected_real = read("real.f32");
        let expected_imag = read("imag.f32");
        let (real, imag) = analyze(&pcm, &|| true).unwrap();
        let max = real
            .iter()
            .chain(&imag)
            .zip(expected_real.iter().chain(&expected_imag))
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert_eq!(real.len(), expected_real.len());
        assert_eq!(imag.len(), expected_imag.len());
        eprintln!("Rust / torch STFT max absolute difference: {max}");
        assert!(max < 1e-3);
        let synthesized = synthesize(&expected_real, &expected_imag, &|| true).unwrap();
        let max = synthesized
            .iter()
            .zip(&pcm)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        eprintln!("Rust iSTFT / source PCM max absolute difference: {max}");
        assert!(max < 1e-6);
    }

    #[test]
    fn stereo_roundtrip_preserves_gain_phase_endpoints_and_center_padding() {
        let mut pcm = vec![0.; FRAMES * 2];
        for i in 0..FRAMES {
            pcm[i * 2] = (i as f32 * 0.071).sin() * 0.3;
            pcm[i * 2 + 1] = (i as f32 * 0.043).cos() * 0.2;
        }
        pcm[0] = 0.8;
        pcm[FRAMES * 2 - 1] = -0.7;
        let (real, imag) = analyze(&pcm, &|| true).unwrap();
        let out = synthesize(&real, &imag, &|| true).unwrap();
        assert_eq!(out.len(), pcm.len());
        let max = out
            .iter()
            .zip(&pcm)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(max < 1e-6, "{max}");
        // DC and periodic Hann match torch.stft(center=True, pad_mode=reflect).
        let constant = vec![0.25; FRAMES * 2];
        let (re, im) = analyze(&constant, &|| true).unwrap();
        assert!((re[0] - 256.).abs() < 1e-3);
        assert!(im[0].abs() < 1e-5);
        assert!((re[STEPS] + 128.).abs() < 1e-3);
        assert!(analyze(&pcm, &|| false).is_err());
    }
}
