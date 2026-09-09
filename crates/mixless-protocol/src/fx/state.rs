//! Effect state, defaults, beat divisions and command parameter conversion.

use serde::{Deserialize, Serialize};

use super::FxKind;

pub const FX_BEATS: [f32; 13] = [
    0.0625, 0.125, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 8.0, 16.0, 32.0,
];
pub const FX_BEAT_LABELS: [&str; 13] = [
    "1/16", "1/8", "1/4", "1/2", "3/4", "1", "3/2", "2", "3", "4", "8", "16", "32",
];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FxState {
    pub kind: FxKind,
    pub on: bool,
    pub mix: f32,
    pub beats: f32,
    /// Zero selects the deck beat clock; positive values select a free LFO.
    pub rate_hz: f32,
    pub feedback: f32,
    pub depth: f32,
    pub drive: f32,
    pub decay_seconds: f32,
    pub size: f32,
    pub damping: f32,
}

impl FxState {
    pub fn new(kind: FxKind) -> Self {
        Self {
            kind,
            beats: kind.default_beats(),
            depth: if matches!(kind, FxKind::Pitch | FxKind::PitchDelay | FxKind::Spiral) {
                0.75
            } else {
                0.5
            },
            ..Self::default()
        }
    }

    pub fn params(self) -> crate::FxParams {
        crate::FxParams {
            kind: Some(self.kind.as_str().into()),
            mix: self.mix,
            time_beats: Some(self.beats),
            rate_hz: Some(self.rate_hz),
            feedback: Some(self.feedback),
            depth: Some(self.depth),
            drive: Some(self.drive),
            decay_seconds: Some(self.decay_seconds),
            size: Some(self.size),
            damping: Some(self.damping),
            ..crate::FxParams::default()
        }
    }
}

impl Default for FxState {
    fn default() -> Self {
        Self {
            kind: FxKind::Echo,
            on: false,
            mix: 0.5,
            beats: 0.5,
            rate_hz: 0.0,
            feedback: 0.35,
            depth: 0.5,
            drive: 0.5,
            decay_seconds: 1.6,
            size: 0.5,
            damping: 0.5,
        }
    }
}

pub fn default_fx() -> [FxState; 4] {
    [FxKind::Echo, FxKind::Flanger, FxKind::Gate, FxKind::Reverb].map(FxState::new)
}
