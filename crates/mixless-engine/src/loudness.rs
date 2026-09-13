//! Offline gated BS.1770 loudness. A constant source trim preserves dynamics;
//! it never rides a quiet intro or rewrites the user's audio file.
use ebur128::{EbuR128, Mode};

pub const TARGET_LUFS: f64 = -18.;
const PEAK_CEILING_DB: f64 = -1.;

#[derive(Debug, Clone, Copy)]
pub struct Loudness {
    pub integrated_lufs: Option<f32>,
    pub true_peak_db: Option<f32>,
    pub gain_db: f32,
    pub gain: f32,
}

impl Default for Loudness {
    fn default() -> Self {
        Self {
            integrated_lufs: None,
            true_peak_db: None,
            gain_db: 0.,
            gain: 1.,
        }
    }
}

impl Loudness {
    pub fn measure(samples: &[f32], sample_rate: u32) -> Self {
        Self::try_measure(samples, sample_rate).unwrap_or_else(|error| {
            tracing::warn!(?error, "Loudness unavailable; retaining source level");
            Self::default()
        })
    }

    fn try_measure(samples: &[f32], sample_rate: u32) -> Result<Self, ebur128::Error> {
        let mut meter = EbuR128::new(2, sample_rate, Mode::I | Mode::TRUE_PEAK)?;
        meter.add_frames_f32(samples)?;
        let integrated = meter.loudness_global()?;
        let peak = meter.true_peak(0)?.max(meter.true_peak(1)?);
        let peak_db = 20. * peak.log10();
        // Below the absolute loudness gate and sub-400 ms clips have no usable
        // integrated reading. Never amplify silence/noise to a target.
        let requested = if integrated.is_finite() {
            (TARGET_LUFS - integrated).clamp(-36., 12.)
        } else {
            0.
        };
        let gain_db = if peak_db.is_finite() {
            requested.min(PEAK_CEILING_DB - peak_db)
        } else {
            0.
        } as f32;
        Ok(Self {
            integrated_lufs: integrated.is_finite().then_some(integrated as f32),
            true_peak_db: peak_db.is_finite().then_some(peak_db as f32),
            gain_db,
            gain: 10f32.powf(gain_db / 20.),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tone(amplitude: f32) -> Vec<f32> {
        (0..96_000)
            .flat_map(|i| {
                let x = amplitude * (std::f32::consts::TAU * 1000. * i as f32 / 48_000.).sin();
                [x, -x] // Anti-phase stereo still has full loudness.
            })
            .collect()
    }
    #[test]
    fn different_masters_reach_the_same_loudness_without_changing_dynamics() {
        for amplitude in [0.08, 0.8] {
            let samples = tone(amplitude);
            let m = Loudness::measure(&samples, 48_000);
            let output: Vec<_> = samples.iter().map(|x| x * m.gain).collect();
            let measured = Loudness::measure(&output, 48_000);
            assert!((measured.integrated_lufs.unwrap() + 18.).abs() < 0.1);
            assert!(measured.true_peak_db.unwrap() <= -0.99);
        }
    }
    #[test]
    fn silence_short_clips_and_large_transients_are_safe() {
        let silence = Loudness::measure(&vec![0.; 96_000], 48_000);
        assert_eq!(silence.gain, 1.);
        let short = Loudness::measure(&vec![0.001; 400], 48_000);
        assert!(short.gain <= 1.);
        let mut transient = tone(0.02);
        transient[48_000] = 1.5;
        let m = Loudness::measure(&transient, 48_000);
        assert!(m.true_peak_db.unwrap() + m.gain_db <= -0.99);
    }
}
