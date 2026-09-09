//! FX control labels, normalized values and per-kind parameter selection.

use mixless_protocol::{FxKind, FxState};

#[derive(Debug, Clone, Copy)]
pub enum FxParam {
    Rate,
    Feedback,
    Depth,
    Drive,
    Decay,
    Size,
    Damping,
}

impl FxParam {
    pub fn label(self, kind: FxKind) -> &'static str {
        match self {
            Self::Rate => "RATE",
            Self::Feedback => "FEEDBACK",
            Self::Decay => "DECAY",
            Self::Size => match kind {
                FxKind::Filter
                | FxKind::AutoFilter
                | FxKind::LowPass
                | FxKind::HighPass
                | FxKind::BandPass
                | FxKind::VowelFilter => "RESONANCE",
                FxKind::Granulizer => "GRAIN SIZE",
                _ => "SIZE",
            },
            Self::Damping => "DAMPING",
            Self::Depth => match kind {
                FxKind::Gate | FxKind::Transform => "WIDTH",
                FxKind::Pitch | FxKind::PitchDelay | FxKind::Spiral | FxKind::Granulizer => "PITCH",
                FxKind::LowPass | FxKind::HighPass | FxKind::BandPass => "CUTOFF",
                FxKind::Compressor | FxKind::NoiseGate => "THRESHOLD",
                FxKind::Robot | FxKind::Resonator => "TUNE",
                FxKind::Crush => "DOWNSAMPLE",
                _ => "DEPTH",
            },
            Self::Drive => match kind {
                FxKind::Crush => "BITS",
                FxKind::Noise
                | FxKind::NoiseSweep
                | FxKind::NoisePump
                | FxKind::GatedNoise
                | FxKind::NoiseFollower
                | FxKind::Riser => "LEVEL",
                FxKind::RingMod | FxKind::Vocoder => "CARRIER",
                _ => "DRIVE",
            },
        }
    }

    pub fn value(self, fx: &FxState) -> f32 {
        match self {
            Self::Rate => (fx.rate_hz.max(0.05) / 0.05).ln() / 400.0f32.ln(),
            Self::Feedback => fx.feedback / 0.88,
            Self::Depth => fx.depth,
            Self::Drive => fx.drive,
            Self::Decay => (fx.decay_seconds / 0.2).ln() / 40.0f32.ln(),
            Self::Size => fx.size,
            Self::Damping => fx.damping,
        }
        .clamp(0.0, 1.0)
    }

    pub fn set(self, fx: &mut FxState, value: f32) {
        let value = value.clamp(0.0, 1.0);
        match self {
            Self::Rate => fx.rate_hz = 0.05 * 400.0f32.powf(value),
            Self::Feedback => fx.feedback = value * 0.88,
            Self::Depth => fx.depth = value,
            Self::Drive => fx.drive = value,
            Self::Decay => fx.decay_seconds = 0.2 * 40.0f32.powf(value),
            Self::Size => fx.size = value,
            Self::Damping => fx.damping = value,
        }
    }

    pub fn display(self, fx: &FxState) -> String {
        match self {
            Self::Rate => format!("{:.2} Hz", fx.rate_hz),
            Self::Feedback => format!("{:.0}%", fx.feedback * 100.0),
            Self::Decay => format!("{:.2} s", fx.decay_seconds),
            Self::Depth
                if matches!(
                    fx.kind,
                    FxKind::Pitch | FxKind::PitchDelay | FxKind::Spiral | FxKind::Granulizer
                ) =>
            {
                format!("{:+.1} st", fx.depth * 24.0 - 12.0)
            }
            Self::Depth
                if matches!(
                    fx.kind,
                    FxKind::LowPass | FxKind::HighPass | FxKind::BandPass
                ) =>
            {
                format!("{:.0} Hz", 30.0 * 600.0f32.powf(fx.depth))
            }
            Self::Depth if fx.kind == FxKind::Compressor => {
                format!("{:.0} dB", -6.0 - fx.depth * 30.0)
            }
            Self::Depth if fx.kind == FxKind::NoiseGate => {
                format!("{:.0} dB", -60.0 + fx.depth * 50.0)
            }
            Self::Drive if fx.kind == FxKind::RingMod => {
                format!("{:.0} Hz", 20.0 * 100.0f32.powf(fx.drive))
            }
            Self::Drive if fx.kind == FxKind::Vocoder => {
                format!("{:.0} Hz", 55.0 * 8.0f32.powf(fx.drive))
            }
            Self::Depth if fx.kind == FxKind::Gate => format!("{:.0}%", 10.0 + fx.depth * 80.0),
            Self::Depth if fx.kind == FxKind::Crush => {
                format!("1/{:.0}", 1.0 + (1.0 - fx.depth) * 15.0)
            }
            Self::Drive if fx.kind == FxKind::Crush => format!("{:.0} bit", 4.0 + fx.drive * 12.0),
            _ => format!("{:.0}%", self.value(fx) * 100.0),
        }
    }
}

impl FxParam {
    pub(super) fn for_kind(kind: FxKind) -> Vec<Self> {
        let mut params = Vec::new();
        if kind.is_modulated()
            || matches!(kind, FxKind::Jet | FxKind::Mobius | FxKind::MobiusTriangle)
        {
            params.push(Self::Rate);
        }
        if kind.has_feedback() {
            params.push(Self::Feedback);
        }
        if !matches!(
            kind,
            FxKind::Delay
                | FxKind::Echo
                | FxKind::PingPong
                | FxKind::EchoOut
                | FxKind::TapeStop
                | FxKind::VinylBrake
                | FxKind::Backspin
                | FxKind::OneShot
                | FxKind::Fader
                | FxKind::Roll
                | FxKind::Reverse
                | FxKind::ReverseRoll
                | FxKind::BeatLoop
                | FxKind::SlipRoll
                | FxKind::LoopRoll
                | FxKind::Censor
                | FxKind::Slicer
                | FxKind::Beatmasher
                | FxKind::Helix
        ) && !kind.is_reverb()
        {
            params.push(Self::Depth);
        }
        if kind.has_drive() {
            params.push(Self::Drive);
        }
        if kind.is_reverb() || kind == FxKind::Macro {
            params.extend([Self::Decay, Self::Size, Self::Damping]);
        } else if matches!(
            kind,
            FxKind::Granulizer
                | FxKind::Filter
                | FxKind::AutoFilter
                | FxKind::LowPass
                | FxKind::HighPass
                | FxKind::BandPass
        ) {
            params.push(Self::Size);
        }
        params
    }
}
