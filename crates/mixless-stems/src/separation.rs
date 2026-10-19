//! One BS-RoFormer call predicts all four stems for each overlapping chunk.
use crate::{check, spectral, Error, Inference, Progress, Result, Stems};
use ort::value::Tensor;
const FRAMES: usize = spectral::FRAMES;
const STRIDE: usize = FRAMES / 2;
const FADE: usize = FRAMES / 10;

impl Inference {
    pub fn separate(
        &mut self,
        input: &[f32],
        sr: u32,
        progress: &mut impl FnMut(Progress),
        active: &impl Fn() -> bool,
    ) -> Result<Stems> {
        if sr == 0
            || input.is_empty()
            || input.len() % 2 != 0
            || input.iter().any(|v| !v.is_finite())
        {
            return Err(Error::Model("Invalid stereo audio for separation".into()));
        }
        check(active)?;
        let mix = crate::resample::convert(input, 2, sr, 44100);
        let total = mix.len() / 2;
        let border = if total > 2 * (FRAMES - STRIDE) {
            FRAMES - STRIDE
        } else {
            0
        };
        let padded = total + border * 2;
        let mut audio: [Vec<f32>; 3] = std::array::from_fn(|_| vec![0.; mix.len()]);
        let mut bass = vec![0.; mix.len()];
        let mut weights = vec![0.; total];
        for start in (0..padded).step_by(STRIDE) {
            check(active)?;
            progress(Progress::Separating((100 * start / padded.max(1)) as u8));
            let valid = FRAMES.min(padded - start);
            let mut block = vec![0.; FRAMES * 2];
            for i in 0..FRAMES {
                // Match MSST chunk padding: reflect a long tail, zero-pad a short one.
                if i >= valid && valid <= FRAMES / 2 {
                    break;
                }
                let local = if i < valid {
                    i
                } else {
                    spectral::reflect(i as isize, valid)
                };
                let at = spectral::reflect((start + local) as isize - border as isize, total);
                block[i * 2..i * 2 + 2].copy_from_slice(&mix[at * 2..at * 2 + 2]);
            }
            let (real, imag) = spectral::analyze(&block, active)?;
            let outputs = self.separator.run_inputs(
                vec![
                    (
                        "spec_real".into(),
                        Tensor::from_array(([1usize, 2, spectral::BINS, spectral::STEPS], real))?,
                    ),
                    (
                        "spec_imag".into(),
                        Tensor::from_array(([1usize, 2, spectral::BINS, spectral::STEPS], imag))?,
                    ),
                ],
                &["out_spec_real", "out_spec_imag"],
                active,
            )?;
            let (shape, real) = outputs[0].try_extract_tensor::<f32>()?;
            let (ishape, imag) = outputs[1].try_extract_tensor::<f32>()?;
            if shape.as_ref() != [1, 4, 2, spectral::BINS as i64, spectral::STEPS as i64]
                || shape != ishape
            {
                return Err(Error::Model("Invalid BS-RoFormer output shape".into()));
            }
            for stem in 0..4 {
                let offset = stem * spectral::SPECTRUM;
                let pcm = spectral::synthesize(
                    &real[offset..offset + spectral::SPECTRUM],
                    &imag[offset..offset + spectral::SPECTRUM],
                    active,
                )?;
                for i in 0..valid {
                    let absolute = start + i;
                    if absolute < border || absolute >= border + total {
                        continue;
                    }
                    let frame = absolute - border;
                    let weight = window(i, start == 0, start + STRIDE >= padded);
                    if stem == 0 {
                        weights[frame] += weight;
                    }
                    for c in 0..2 {
                        let at = frame * 2 + c;
                        let sample = pcm[i * 2 + c] * weight;
                        match stem {
                            0 => audio[1][at] += sample,
                            1 => {
                                bass[at] += sample;
                                audio[2][at] += sample;
                            }
                            2 => audio[2][at] += sample,
                            3 => audio[0][at] += sample,
                            _ => unreachable!(),
                        }
                    }
                }
            }
        }
        let mut residual = 0f64;
        for i in 0..total {
            if weights[i] <= 0. {
                return Err(Error::Model("Uncovered stem sample".into()));
            }
            for c in 0..2 {
                let at = i * 2 + c;
                bass[at] /= weights[i];
                for stem in &mut audio {
                    stem[at] /= weights[i];
                }
                residual += (mix[at] - audio.iter().map(|s| s[at]).sum::<f32>()).powi(2) as f64;
            }
        }
        progress(Progress::Separating(100));
        Ok(Stems {
            audio,
            bass,
            residual_rms: (residual / mix.len().max(1) as f64).sqrt() as f32,
        })
    }
}
fn window(i: usize, first: bool, last: bool) -> f32 {
    if !first && i < FADE {
        i as f32 / (FADE - 1) as f32
    } else if !last && i >= FRAMES - FADE {
        (FRAMES - 1 - i) as f32 / (FADE - 1) as f32
    } else {
        1.
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunk_overlap_has_no_uncovered_source_samples() {
        for total in [1, 100, FRAMES / 2, FRAMES, FRAMES + 17, FRAMES * 3] {
            let border = if total > 2 * (FRAMES - STRIDE) {
                FRAMES - STRIDE
            } else {
                0
            };
            let padded = total + border * 2;
            let mut weights = vec![0.; total];
            for start in (0..padded).step_by(STRIDE) {
                for i in 0..FRAMES.min(padded - start) {
                    let at = start + i;
                    if at >= border && at < border + total {
                        weights[at - border] += window(i, start == 0, start + STRIDE >= padded);
                    }
                }
            }
            assert!(weights.iter().all(|w| w.is_finite() && *w > 0.), "{total}");
        }
    }
}
