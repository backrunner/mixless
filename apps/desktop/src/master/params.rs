use mixless_protocol::MasterChainSettings;

#[derive(Clone, Copy, Debug)]
pub enum MasterParam {
    Frequency(usize),
    EqGain(usize),
    Q(usize),
    Threshold,
    Ratio,
    Knee,
    Attack,
    Release,
    Makeup,
    GateThreshold,
    GateRatio,
    Range,
    GateAttack,
    Hold,
    GateRelease,
    Ceiling,
    LimiterRelease,
}

impl MasterParam {
    pub fn label(self) -> &'static str {
        use MasterParam::*;
        match self {
            Frequency(_) => "FREQ",
            EqGain(_) => "GAIN",
            Q(_) => "Q",
            Threshold | GateThreshold => "THRESHOLD",
            Ratio | GateRatio => "RATIO",
            Knee => "KNEE",
            Attack | GateAttack => "ATTACK",
            Release | GateRelease | LimiterRelease => "RELEASE",
            Makeup => "MAKEUP",
            Range => "RANGE",
            Hold => "HOLD",
            Ceiling => "CEILING",
        }
    }
    fn bounds(self) -> (f32, f32, bool) {
        use MasterParam::*;
        match self {
            Frequency(_) => (20., 20000., true),
            EqGain(_) => (-12., 12., false),
            Q(_) => (0.2, 10., true),
            Threshold => (-60., 0., false),
            GateThreshold => (-80., 0., false),
            Ratio => (1., 20., true),
            GateRatio => (1., 10., true),
            Knee => (0., 24., false),
            Attack => (0.1, 200., true),
            GateAttack => (0.1, 100., true),
            Release | GateRelease => (10., 2000., true),
            LimiterRelease => (10., 1000., true),
            Makeup => (0., 12., false),
            Range => (0., 80., false),
            Hold => (0., 500., false),
            Ceiling => (-12., -0.1, false),
        }
    }
    fn field(self, p: &mut MasterChainSettings) -> &mut f32 {
        use MasterParam::*;
        match self {
            Frequency(i) => &mut p.eq[i].frequency_hz,
            EqGain(i) => &mut p.eq[i].gain_db,
            Q(i) => &mut p.eq[i].q,
            Threshold => &mut p.compressor.threshold_db,
            Ratio => &mut p.compressor.ratio,
            Knee => &mut p.compressor.knee_db,
            Attack => &mut p.compressor.attack_ms,
            Release => &mut p.compressor.release_ms,
            Makeup => &mut p.compressor.makeup_db,
            GateThreshold => &mut p.dynamics.threshold_db,
            GateRatio => &mut p.dynamics.ratio,
            Range => &mut p.dynamics.range_db,
            GateAttack => &mut p.dynamics.attack_ms,
            Hold => &mut p.dynamics.hold_ms,
            GateRelease => &mut p.dynamics.release_ms,
            Ceiling => &mut p.limiter.ceiling_db,
            LimiterRelease => &mut p.limiter.release_ms,
        }
    }
    pub fn value(self, mut p: MasterChainSettings) -> f32 {
        let v = *self.field(&mut p);
        let (lo, hi, log) = self.bounds();
        if log {
            (v / lo).ln() / (hi / lo).ln()
        } else {
            (v - lo) / (hi - lo)
        }
    }
    pub fn set(self, p: &mut MasterChainSettings, value: f32) {
        let (lo, hi, log) = self.bounds();
        let v = value.clamp(0., 1.);
        *self.field(p) = if log {
            lo * (hi / lo).powf(v)
        } else {
            lo + (hi - lo) * v
        }
        .clamp(lo, hi);
    }
    pub fn display(self, mut p: MasterChainSettings) -> String {
        let v = *self.field(&mut p);
        use MasterParam::*;
        match self {
            Frequency(_) if v >= 1000. => format!("{:.2} kHz", v / 1000.),
            Frequency(_) => format!("{v:.0} Hz"),
            Q(_) => format!("{v:.2}"),
            Ratio | GateRatio => format!("{v:.1}:1"),
            Attack | Release | GateAttack | Hold | GateRelease | LimiterRelease => {
                format!("{v:.1} ms")
            }
            _ => format!("{v:.1} dB"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn knob_mapping_round_trips_and_stays_valid() {
        use MasterParam::*;
        let mut p = MasterChainSettings::default();
        for param in [
            Frequency(0),
            EqGain(0),
            Q(0),
            Threshold,
            Ratio,
            Knee,
            Attack,
            Release,
            Makeup,
            GateThreshold,
            GateRatio,
            Range,
            GateAttack,
            Hold,
            GateRelease,
            Ceiling,
            LimiterRelease,
        ] {
            for value in [0., 0.25, 0.5, 1.] {
                param.set(&mut p, value);
                assert!(p.is_valid());
                assert!((param.value(p) - value).abs() < 0.00001);
            }
        }
    }
}
