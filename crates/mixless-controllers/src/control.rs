use mixless_protocol::{Command, EngineSnapshot, FxSlot};
use serde::{Deserialize, Serialize};

use crate::{ControlKind, MidiTarget};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiControlMode {
    /// Legacy maps: absolute knobs; two's complement jog / browse encoders.
    #[default]
    Auto,
    Absolute,
    AbsoluteInverted,
    RelativeTwosComplement,
    RelativeBinaryOffset,
    RelativeSignedBit,
}
impl MidiControlMode {
    pub const ALL: [Self; 6] = [
        Self::Auto,
        Self::Absolute,
        Self::AbsoluteInverted,
        Self::RelativeTwosComplement,
        Self::RelativeBinaryOffset,
        Self::RelativeSignedBit,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::Absolute => "Absolute (0–127)",
            Self::AbsoluteInverted => "Inverted (127–0)",
            Self::RelativeTwosComplement => "Relative: 1 / 127",
            Self::RelativeBinaryOffset => "Relative: 65 / 63",
            Self::RelativeSignedBit => "Relative: 1 / 65",
        }
    }
    pub fn relative(self, target: &MidiTarget) -> bool {
        match self {
            Self::Auto => target.kind() == ControlKind::Encoder,
            Self::Absolute | Self::AbsoluteInverted => false,
            _ => true,
        }
    }
    pub fn delta(self, value: u8) -> i16 {
        match self {
            Self::Auto | Self::RelativeTwosComplement => match value {
                0 | 64 => 0,
                1..=63 => value as i16,
                _ => value as i16 - 128,
            },
            Self::RelativeBinaryOffset => value as i16 - 64,
            Self::RelativeSignedBit => {
                if value < 64 {
                    value as i16
                } else {
                    -(value as i16 - 64)
                }
            }
            Self::Absolute | Self::AbsoluteInverted => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MidiValue {
    Absolute(u8),
    Normalized(f64),
    JogFrames(f64),
    Relative(i16),
    Press,
    Hold,
    Release,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MidiAction {
    pub target: MidiTarget,
    pub value: MidiValue,
}

impl MidiValue {
    fn ranged(self, current: f32, min: f32, max: f32) -> f32 {
        match self {
            Self::Absolute(v) => min + v as f32 / 127. * (max - min),
            Self::Normalized(v) => min + v.clamp(0., 1.) as f32 * (max - min),
            Self::Relative(v) => (current + v as f32 / 127. * (max - min)).clamp(min, max),
            _ => current,
        }
    }
    pub fn steps(self) -> i16 {
        match self {
            Self::Relative(v) => v,
            _ => 0,
        }
    }
}

impl MidiAction {
    /// Desktop-only actions (library, cues, AutoMix, FX selection) are handled
    /// by UiState. Audio parameters resolve against a fresh engine snapshot.
    pub fn command(&self, snapshot: &EngineSnapshot) -> Option<Command> {
        use MidiTarget::*;
        let d = self.target.deck().map(|deck| snapshot.deck(deck));
        let range = |current, min, max| self.value.ranged(current, min, max);
        let pressed = self.value != MidiValue::Release;
        Some(match self.target {
            Xfader => Command::SetCrossfader {
                value: range(snapshot.xfader, -1., 1.),
            },
            Master => Command::SetMaster {
                value: range(snapshot.master, 0., 1.),
            },
            MasterGain => Command::SetMasterGain {
                db: range(snapshot.master_gain_db, -12., 12.),
            },
            CueMix => Command::SetCueMix {
                value: range(snapshot.cue_mix, 0., 1.),
            },
            CueGain => Command::SetCueGain {
                value: range(snapshot.cue_gain, 0., 1.),
            },
            Quantize => Command::SetQuantize {
                on: !snapshot.quantize,
            },
            XfReverse => Command::SetXfReverse {
                on: !snapshot.xf_reverse,
            },
            Fader { deck } => Command::SetChannelFader {
                deck,
                value: range(d?.fader, 0., 1.),
            },
            Gain { deck } => Command::SetChannelGain {
                deck,
                db: range(d?.gain_db, -12., 12.),
            },
            DeckGain { deck } => Command::SetDeckLimiterGain {
                deck,
                db: range(d?.effective_limiter_gain_db(), -12., 12.),
            },
            Balance { deck } => Command::SetBalance {
                deck,
                value: range(d?.balance, -1., 1.),
            },
            Eq { deck, band } => Command::SetEq {
                deck,
                band,
                db: range(d?.eq_db[band.index()], -12., 12.),
            },
            EqKill { deck, band } => Command::SetEqKill {
                deck,
                band,
                on: !d?.eq_kill[band.index()],
            },
            Filter { deck } => Command::SetChannelFilter {
                deck,
                amount: range(d?.filter_amount, -1., 1.),
            },
            Resonance { deck } => Command::SetFilterResonance {
                deck,
                resonance: range(d?.filter_resonance, 0., 1.),
            },
            ResonanceEnabled { deck } => Command::SetFilterResonanceEnabled {
                deck,
                on: !d?.filter_resonance_enabled,
            },
            Tempo { deck } => Command::SetRate {
                deck,
                rate: range(d?.rate, 0.88, 1.12),
            },
            Pitch { deck } => Command::SetPitchSemitones {
                deck,
                semitones: range(d?.pitch_semitones, -6., 6.),
            },
            Position { deck } => {
                let d = d?;
                let maximum = d.frames.saturating_sub(1);
                let current = if maximum > 0 {
                    d.frame as f32 / maximum as f32
                } else {
                    0.
                };
                Command::Seek {
                    deck,
                    frame: (range(current, 0., 1.) as f64 * maximum as f64).round() as u64,
                }
            }
            Jog { deck } => Command::Jog {
                deck,
                delta_frames: match self.value {
                    MidiValue::JogFrames(v) => v as f32,
                    _ => self.value.steps() as f32 * 16.,
                },
            },
            JogTouch { deck } => Command::SetJogTouch {
                deck,
                touching: pressed,
            },
            KeyLock { deck } => Command::SetKeyLock {
                deck,
                on: !d?.keylock,
            },
            Vinyl { deck } => Command::SetVinylMode {
                deck,
                vinyl: !d?.vinyl,
                slip: d?.slip,
            },
            Slip { deck } => Command::SetVinylMode {
                deck,
                vinyl: d?.vinyl,
                slip: !d?.slip,
            },
            Reverse { deck } => Command::SetReverse {
                deck,
                on: !d?.reverse,
            },
            Brake { deck } => Command::SetBrake { deck, on: pressed },
            Pfl { deck } => Command::SetPfl { deck, on: !d?.pfl },
            Loop { deck } => Command::SetLoopBeats {
                deck,
                beats: d?.loop_beats,
                on: !d?.loop_on,
            },
            LoopHalve { deck } => Command::LoopHalve { deck },
            LoopDouble { deck } => Command::LoopDouble { deck },
            BeatBack { deck } => Command::BeatJump { deck, bars: -1 },
            BeatForward { deck } => Command::BeatJump { deck, bars: 1 },
            Stem { deck, stem } => Command::SetStemGain {
                deck,
                stem,
                value: range(d?.stem_gain[stem.index()], 0., 1.),
            },
            FxOn { deck, slot } => Command::SetFxBypass {
                deck,
                slot: *FxSlot::INSERTS.get(slot as usize)?,
                on: d?.fx.get(slot as usize)?.on,
            },
            AutomixPause if snapshot.automix_on => {
                if snapshot.automix_paused {
                    Command::ResumeAutomix
                } else {
                    Command::PauseAutomix
                }
            }
            AutomixSkip if snapshot.automix_on => Command::SkipAutomix,
            FxMix { deck, slot } => {
                let mut fx = *d?.fx.get(slot as usize)?;
                fx.mix = range(fx.mix, 0., 1.);
                Command::SetFx {
                    deck: Some(deck),
                    slot: *FxSlot::INSERTS.get(slot as usize)?,
                    params: fx.params(),
                }
            }
            FxSend { deck } => Command::SetFxSend {
                deck,
                value: range(d?.send, 0., 1.),
            },
            _ => return None,
        })
    }
}
