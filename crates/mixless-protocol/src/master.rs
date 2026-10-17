//! Master-bus controls, shared transfer curves and callback telemetry.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MasterEqBand {
    pub frequency_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterCompressor {
    pub enabled: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
}

impl Default for MasterCompressor {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -18.,
            ratio: 2.,
            knee_db: 6.,
            attack_ms: 20.,
            release_ms: 160.,
            makeup_db: 0.,
        }
    }
}

impl MasterCompressor {
    /// Positive attenuation, before makeup. Same curve in the DSP and editor.
    pub fn reduction_db(self, input_db: f32) -> f32 {
        let over = input_db - self.threshold_db;
        let slope = 1. - self.ratio.recip();
        if self.knee_db > 0. && over.abs() < self.knee_db * 0.5 {
            slope * (over + self.knee_db * 0.5).powi(2) / (2. * self.knee_db)
        } else {
            slope * over.max(0.)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterDynamics {
    pub enabled: bool,
    pub gate: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub range_db: f32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub release_ms: f32,
}

impl Default for MasterDynamics {
    fn default() -> Self {
        Self {
            enabled: false,
            gate: false,
            threshold_db: -48.,
            ratio: 2.,
            range_db: 36.,
            attack_ms: 2.,
            hold_ms: 50.,
            release_ms: 180.,
        }
    }
}

impl MasterDynamics {
    pub fn reduction_db(self, input_db: f32) -> f32 {
        if input_db >= self.threshold_db {
            0.
        } else if self.gate {
            self.range_db
        } else {
            ((self.threshold_db - input_db) * (self.ratio - 1.)).min(self.range_db)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterLimiterSettings {
    pub ceiling_db: f32,
    pub release_ms: f32,
}

impl Default for MasterLimiterSettings {
    fn default() -> Self {
        Self {
            ceiling_db: 20. * 0.98_f32.log10(),
            release_ms: 80.,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterChainSettings {
    pub eq_enabled: bool,
    pub eq: [MasterEqBand; 5],
    pub compressor: MasterCompressor,
    pub dynamics: MasterDynamics,
    pub limiter: MasterLimiterSettings,
}

impl Default for MasterChainSettings {
    fn default() -> Self {
        Self {
            eq_enabled: false,
            eq: [60., 250., 1000., 4000., 12000.].map(|frequency_hz| MasterEqBand {
                frequency_hz,
                gain_db: 0.,
                q: 0.8,
            }),
            compressor: MasterCompressor::default(),
            dynamics: MasterDynamics::default(),
            limiter: MasterLimiterSettings::default(),
        }
    }
}

impl MasterChainSettings {
    pub fn is_valid(&self) -> bool {
        let valid = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
        let c = self.compressor;
        let d = self.dynamics;
        self.eq.iter().all(|b| {
            valid(b.frequency_hz, 20., 20000.)
                && valid(b.gain_db, -12., 12.)
                && valid(b.q, 0.2, 10.)
        }) && valid(c.threshold_db, -60., 0.)
            && valid(c.ratio, 1., 20.)
            && valid(c.knee_db, 0., 24.)
            && valid(c.attack_ms, 0.1, 200.)
            && valid(c.release_ms, 10., 2000.)
            && valid(c.makeup_db, 0., 12.)
            && valid(d.threshold_db, -80., 0.)
            && valid(d.ratio, 1., 10.)
            && valid(d.range_db, 0., 80.)
            && valid(d.attack_ms, 0.1, 100.)
            && valid(d.hold_ms, 0., 500.)
            && valid(d.release_ms, 10., 2000.)
            && valid(self.limiter.ceiling_db, -12., -0.1)
            && valid(self.limiter.release_ms, 10., 1000.)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MasterStageMeter {
    pub input: [f32; 2],
    pub output: [f32; 2],
    pub reduction_db: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MasterChainMeters {
    pub compressor: MasterStageMeter,
    pub dynamics: MasterStageMeter,
    pub limiter: MasterStageMeter,
    /// RMS at the limiter output, before the independent output level.
    pub rms: [f32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_legacy_and_invalid_values() {
        let p: MasterChainSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(p, MasterChainSettings::default());
        assert!(p.is_valid());
        let mut bad = p;
        bad.eq[0].q = f32::NAN;
        assert!(!bad.is_valid());
        bad = p;
        bad.compressor.ratio = 0.;
        assert!(!bad.is_valid());
        bad = p;
        bad.limiter.ceiling_db = 1.;
        assert!(!bad.is_valid());
    }
    #[test]
    fn compressor_knee_and_expander_range() {
        let c = MasterCompressor {
            knee_db: 0.,
            ..Default::default()
        };
        assert_eq!(c.reduction_db(-24.), 0.);
        assert_eq!(c.reduction_db(-6.), 6.);
        let d = MasterDynamics::default();
        assert_eq!(d.reduction_db(-40.), 0.);
        assert_eq!(d.reduction_db(-54.), 6.);
        assert_eq!(d.reduction_db(-120.), d.range_db);
    }
}
