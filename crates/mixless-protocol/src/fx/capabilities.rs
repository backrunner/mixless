//! Timing limits and DSP capabilities shared by the engine and UI.

use super::FxKind;

impl FxKind {
    pub fn is_delay(self) -> bool {
        matches!(
            self,
            Self::Echo
                | Self::PingPong
                | Self::DubEcho
                | Self::Delay
                | Self::EchoOut
                | Self::SpaceEcho
                | Self::LowCutEcho
                | Self::TapeEcho
                | Self::PatternDelay
                | Self::PitchDelay
                | Self::Spiral
                | Self::ReverseDelay
        )
    }
    pub fn is_capture(self) -> bool {
        matches!(
            self,
            Self::Roll
                | Self::SlipRoll
                | Self::LoopRoll
                | Self::Beatmasher
                | Self::Stutter
                | Self::VinylBrake
                | Self::TapeStop
                | Self::Backspin
                | Self::BeatLoop
                | Self::OneShot
                | Self::Reverse
                | Self::Slicer
                | Self::Censor
                | Self::ReverseRoll
                | Self::Helix
                | Self::ReverseDelay
        )
    }
    pub fn is_modulated(self) -> bool {
        matches!(
            self,
            Self::Gate
                | Self::Transform
                | Self::Flanger
                | Self::Phaser
                | Self::Chorus
                | Self::Tremolo
                | Self::AutoPan
                | Self::Filter
                | Self::Lfo
                | Self::AutoSidechain
                | Self::GatedNoise
                | Self::NoisePump
                | Self::Jet
                | Self::Mobius
                | Self::MobiusTriangle
        )
    }
    pub fn has_feedback(self) -> bool {
        matches!(
            self,
            Self::Echo
                | Self::PingPong
                | Self::DubEcho
                | Self::EchoOut
                | Self::SpaceEcho
                | Self::LowCutEcho
                | Self::TapeEcho
                | Self::PatternDelay
                | Self::PitchDelay
                | Self::Spiral
                | Self::Flanger
                | Self::Phaser
                | Self::Jet
                | Self::Helix
                | Self::ReverseDelay
                | Self::Robot
                | Self::Resonator
                | Self::Mobius
                | Self::MobiusTriangle
        )
    }
    pub fn has_drive(self) -> bool {
        matches!(
            self,
            Self::Crush
                | Self::Dist
                | Self::Noise
                | Self::Fuzz
                | Self::Overdrive
                | Self::RingMod
                | Self::Vocoder
                | Self::NoiseSweep
                | Self::GatedNoise
                | Self::NoisePump
                | Self::NoiseFollower
                | Self::Riser
                | Self::Macro
        )
    }
    pub fn is_reverb(self) -> bool {
        matches!(
            self,
            Self::Reverb | Self::Space | Self::FreezeVerb | Self::ReverbOut
        )
    }
    pub fn is_release(self) -> bool {
        matches!(
            self,
            Self::EchoOut
                | Self::ReverbOut
                | Self::VinylBrake
                | Self::TapeStop
                | Self::Backspin
                | Self::Riser
                | Self::OneShot
                | Self::Fader
        )
    }
    pub fn has_tail(self) -> bool {
        self.is_delay() || self.is_reverb()
    }
    pub fn max_beats(self) -> f32 {
        if self.is_delay() || self.is_capture() {
            4.0
        } else {
            32.0
        }
    }
    pub fn default_beats(self) -> f32 {
        if self.is_delay()
            || self.is_capture()
            || matches!(self, Self::Gate | Self::Transform | Self::AutoSidechain)
        {
            0.5
        } else if self.is_release() {
            4.0
        } else {
            8.0
        }
    }
}
