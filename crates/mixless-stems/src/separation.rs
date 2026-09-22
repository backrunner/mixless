use crate::{check, Error, Inference, Progress, Result, Stems};
use ort::value::Tensor;

const FRAMES: usize = 343980;
const OVERLAP: usize = FRAMES / 4;
const STRIDE: usize = FRAMES - OVERLAP;

impl Inference {
    pub fn separate(
        &mut self,
        input: &[f32],
        sr: u32,
        progress: &mut impl FnMut(Progress),
        active: &impl Fn() -> bool,
    ) -> Result<Stems> {
        if sr == 0 || input.len() % 2 != 0 || input.iter().any(|v| !v.is_finite()) {
            return Err(Error::Model("Invalid stereo audio for separation".into()));
        }
        check(active)?;
        let mix = crate::resample::convert(input, 2, sr, 44100);
        let total = mix.len() / 2;
        let mut audio: [Vec<f32>; 3] = std::array::from_fn(|_| vec![0.; mix.len()]);
        let mut weights = vec![0f32; total];
        for start in (0..total).step_by(STRIDE) {
            check(active)?;
            progress(Progress::Separating((100 * start / total.max(1)) as u8));
            let valid = FRAMES.min(total - start);
            let mut block = vec![0f32; FRAMES * 2];
            for i in 0..valid {
                for c in 0..2 {
                    block[c * FRAMES + i] = mix[(start + i) * 2 + c];
                }
            }
            let output = self.separator.run(
                "mix",
                Tensor::from_array(([1usize, 2, FRAMES], block))?,
                &["stems"],
                active,
            )?;
            let (shape, values) = output[0].try_extract_tensor::<f32>()?;
            if shape.as_ref() != [1, 4, 2, FRAMES as i64] || values.iter().any(|v| !v.is_finite()) {
                return Err(Error::Model(
                    "Invalid separator output shape or samples".into(),
                ));
            }
            for i in 0..valid {
                // The first/last endpoint has no neighbor: keep unity coverage.
                let weight = window(i, start == 0, start + valid == total);
                weights[start + i] += weight;
                for c in 0..2 {
                    let at = c * FRAMES + i;
                    audio[0][(start + i) * 2 + c] += values[6 * FRAMES + at] * weight;
                    audio[1][(start + i) * 2 + c] += values[at] * weight;
                    audio[2][(start + i) * 2 + c] +=
                        (values[2 * FRAMES + at] + values[4 * FRAMES + at]) * weight;
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
                for stem in &mut audio {
                    stem[at] /= weights[i];
                }
                residual += (mix[at] - audio.iter().map(|s| s[at]).sum::<f32>()).powi(2) as f64;
            }
        }
        progress(Progress::Separating(100));
        Ok(Stems {
            audio,
            residual_rms: (residual / mix.len().max(1) as f64).sqrt() as f32,
        })
    }
}

fn window(i: usize, first: bool, last: bool) -> f32 {
    if !first && i < OVERLAP {
        i as f32 / OVERLAP as f32
    } else if !last && i >= FRAMES - OVERLAP {
        (FRAMES - i) as f32 / OVERLAP as f32
    } else {
        1.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlap_add_has_no_endpoint_holes_and_unity_overlap() {
        assert_eq!(window(0, true, false), 1.);
        assert_eq!(window(FRAMES - 1, false, true), 1.);
        for i in 0..OVERLAP {
            assert!((window(STRIDE + i, true, false) + window(i, false, false) - 1.).abs() < 1e-6);
        }
    }
}
