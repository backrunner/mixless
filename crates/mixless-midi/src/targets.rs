//! The control catalogue is shared by the mapping UI and MIDI dispatch.
use mixless_protocol::{DeckId, EqBand, StemKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    Continuous,
    Encoder,
    Button,
    Momentary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiGroup {
    Mixer,
    DeckA,
    DeckB,
    Library,
}
impl MidiGroup {
    pub const ALL: [Self; 4] = [Self::Mixer, Self::DeckA, Self::DeckB, Self::Library];
    pub fn label(self) -> &'static str {
        match self {
            Self::Mixer => "Mixer / AutoMix",
            Self::DeckA => "Deck A",
            Self::DeckB => "Deck B",
            Self::Library => "Library",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MidiTarget {
    Xfader,
    Master,
    MasterGain,
    CueGain,
    Quantize,
    XfReverse,
    Automix,
    AutomixPause,
    AutomixSkip,
    AutomixShuffle,
    Fader {
        deck: DeckId,
    },
    /// Retains the legacy serialized `gain` mapping to mixer TRIM.
    Gain {
        deck: DeckId,
    },
    DeckGain {
        deck: DeckId,
    },
    Balance {
        deck: DeckId,
    },
    Eq {
        deck: DeckId,
        band: EqBand,
    },
    EqKill {
        deck: DeckId,
        band: EqBand,
    },
    Filter {
        deck: DeckId,
    },
    Resonance {
        deck: DeckId,
    },
    ResonanceEnabled {
        deck: DeckId,
    },
    Tempo {
        deck: DeckId,
    },
    Pitch {
        deck: DeckId,
    },
    Play {
        deck: DeckId,
    },
    TemporaryCue {
        deck: DeckId,
    },
    Cue {
        deck: DeckId,
        index: u8,
    },
    ClearCue {
        deck: DeckId,
        index: u8,
    },
    Jog {
        deck: DeckId,
    },
    JogTouch {
        deck: DeckId,
    },
    Sync {
        deck: DeckId,
    },
    KeyLock {
        deck: DeckId,
    },
    Vinyl {
        deck: DeckId,
    },
    Slip {
        deck: DeckId,
    },
    Reverse {
        deck: DeckId,
    },
    Brake {
        deck: DeckId,
    },
    Pfl {
        deck: DeckId,
    },
    Loop {
        deck: DeckId,
    },
    LoopHalve {
        deck: DeckId,
    },
    LoopDouble {
        deck: DeckId,
    },
    BeatBack {
        deck: DeckId,
    },
    BeatForward {
        deck: DeckId,
    },
    Stem {
        deck: DeckId,
        stem: StemKind,
    },
    FxMix {
        deck: DeckId,
        slot: u8,
    },
    FxOn {
        deck: DeckId,
        slot: u8,
    },
    FxPrevious {
        deck: DeckId,
        slot: u8,
    },
    FxNext {
        deck: DeckId,
        slot: u8,
    },
    FxSend {
        deck: DeckId,
    },
    BrowseTracks,
    BrowsePlaylists,
    TrackPrevious,
    TrackNext,
    PlaylistPrevious,
    PlaylistNext,
    LoadSelected {
        deck: DeckId,
    },
    LoadFocused,
    Focus {
        deck: DeckId,
    },
}

impl MidiTarget {
    pub fn all() -> Vec<Self> {
        let mut targets = vec![
            Self::Xfader,
            Self::Master,
            Self::MasterGain,
            Self::CueGain,
            Self::Quantize,
            Self::XfReverse,
            Self::Automix,
            Self::AutomixPause,
            Self::AutomixSkip,
            Self::AutomixShuffle,
        ];
        for deck in [DeckId::A, DeckId::B] {
            targets.extend([
                Self::Play { deck },
                Self::TemporaryCue { deck },
                Self::Sync { deck },
                Self::KeyLock { deck },
                Self::Vinyl { deck },
                Self::Slip { deck },
                Self::Reverse { deck },
                Self::Brake { deck },
                Self::Jog { deck },
                Self::JogTouch { deck },
                Self::Tempo { deck },
                Self::Pitch { deck },
                Self::DeckGain { deck },
                Self::Balance { deck },
                Self::Fader { deck },
                Self::Gain { deck },
                Self::Pfl { deck },
                Self::Filter { deck },
                Self::Resonance { deck },
                Self::ResonanceEnabled { deck },
                Self::Loop { deck },
                Self::LoopHalve { deck },
                Self::LoopDouble { deck },
                Self::BeatBack { deck },
                Self::BeatForward { deck },
                Self::FxSend { deck },
            ]);
            for band in [EqBand::High, EqBand::Mid, EqBand::Low] {
                targets.extend([Self::Eq { deck, band }, Self::EqKill { deck, band }]);
            }
            targets.extend(
                [StemKind::Vocals, StemKind::Drums, StemKind::Instruments]
                    .map(|stem| Self::Stem { deck, stem }),
            );
            for index in 0..8 {
                targets.extend([Self::Cue { deck, index }, Self::ClearCue { deck, index }]);
            }
            for slot in 0..4 {
                targets.extend([
                    Self::FxOn { deck, slot },
                    Self::FxMix { deck, slot },
                    Self::FxPrevious { deck, slot },
                    Self::FxNext { deck, slot },
                ]);
            }
        }
        targets.extend([
            Self::BrowseTracks,
            Self::BrowsePlaylists,
            Self::TrackPrevious,
            Self::TrackNext,
            Self::PlaylistPrevious,
            Self::PlaylistNext,
            Self::LoadFocused,
            Self::LoadSelected { deck: DeckId::A },
            Self::LoadSelected { deck: DeckId::B },
            Self::Focus { deck: DeckId::A },
            Self::Focus { deck: DeckId::B },
        ]);
        targets
    }

    pub fn deck(&self) -> Option<DeckId> {
        use MidiTarget::*;
        match *self {
            Fader { deck }
            | Gain { deck }
            | DeckGain { deck }
            | Balance { deck }
            | Eq { deck, .. }
            | EqKill { deck, .. }
            | Filter { deck }
            | Resonance { deck }
            | ResonanceEnabled { deck }
            | Tempo { deck }
            | Pitch { deck }
            | Play { deck }
            | TemporaryCue { deck }
            | Cue { deck, .. }
            | ClearCue { deck, .. }
            | Jog { deck }
            | JogTouch { deck }
            | Sync { deck }
            | KeyLock { deck }
            | Vinyl { deck }
            | Slip { deck }
            | Reverse { deck }
            | Brake { deck }
            | Pfl { deck }
            | Loop { deck }
            | LoopHalve { deck }
            | LoopDouble { deck }
            | BeatBack { deck }
            | BeatForward { deck }
            | Stem { deck, .. }
            | FxMix { deck, .. }
            | FxOn { deck, .. }
            | FxPrevious { deck, .. }
            | FxNext { deck, .. }
            | FxSend { deck }
            | LoadSelected { deck }
            | Focus { deck } => Some(deck),
            _ => None,
        }
    }

    pub fn group(&self) -> MidiGroup {
        use MidiTarget::*;
        match self {
            BrowseTracks
            | BrowsePlaylists
            | TrackPrevious
            | TrackNext
            | PlaylistPrevious
            | PlaylistNext
            | LoadSelected { .. }
            | LoadFocused
            | Focus { .. } => MidiGroup::Library,
            _ => match self.deck() {
                Some(DeckId::A) => MidiGroup::DeckA,
                Some(DeckId::B) => MidiGroup::DeckB,
                None => MidiGroup::Mixer,
            },
        }
    }

    pub fn kind(&self) -> ControlKind {
        use MidiTarget::*;
        match self {
            Jog { .. } | BrowseTracks | BrowsePlaylists => ControlKind::Encoder,
            JogTouch { .. } | Brake { .. } => ControlKind::Momentary,
            Xfader
            | Master
            | MasterGain
            | CueGain
            | Fader { .. }
            | Gain { .. }
            | DeckGain { .. }
            | Balance { .. }
            | Eq { .. }
            | Filter { .. }
            | Resonance { .. }
            | Tempo { .. }
            | Pitch { .. }
            | Stem { .. }
            | FxMix { .. }
            | FxSend { .. } => ControlKind::Continuous,
            _ => ControlKind::Button,
        }
    }

    pub fn short_label(&self) -> String {
        use MidiTarget::*;
        match self {
            Xfader => "Crossfader".into(),
            Master => "Master volume".into(),
            MasterGain => "Master gain".into(),
            CueGain => "Headphone volume".into(),
            Quantize => "Quantize".into(),
            XfReverse => "Crossfader reverse".into(),
            Automix => "AutoMix on / off".into(),
            AutomixPause => "AutoMix pause / resume".into(),
            AutomixSkip => "AutoMix skip".into(),
            AutomixShuffle => "AutoMix shuffle".into(),
            Fader { .. } => "Channel fader".into(),
            Gain { .. } => "Trim".into(),
            DeckGain { .. } => "Gain".into(),
            Balance { .. } => "Balance".into(),
            Eq { band, .. } => format!("EQ {band:?}"),
            EqKill { band, .. } => format!("EQ {band:?} kill"),
            Filter { .. } => "Filter".into(),
            Resonance { .. } => "Resonance".into(),
            ResonanceEnabled { .. } => "Resonance on / off".into(),
            Tempo { .. } => "Tempo".into(),
            Pitch { .. } => "Key".into(),
            Play { .. } => "Play / pause".into(),
            TemporaryCue { .. } => "Temporary cue".into(),
            Cue { index, .. } => format!("Hot cue {}", index + 1),
            ClearCue { index, .. } => format!("Clear hot cue {}", index + 1),
            Jog { .. } => "Jog rotation".into(),
            JogTouch { .. } => "Jog touch (hold)".into(),
            Sync { .. } => "Sync".into(),
            KeyLock { .. } => "Key lock".into(),
            Vinyl { .. } => "Vinyl mode".into(),
            Slip { .. } => "Slip mode".into(),
            Reverse { .. } => "Reverse".into(),
            Brake { .. } => "Brake (hold)".into(),
            Pfl { .. } => "Headphone cue / PFL".into(),
            Loop { .. } => "Loop on / off".into(),
            LoopHalve { .. } => "Loop size ÷ 2".into(),
            LoopDouble { .. } => "Loop size × 2".into(),
            BeatBack { .. } => "Beat jump −1 bar".into(),
            BeatForward { .. } => "Beat jump +1 bar".into(),
            Stem { stem, .. } => format!("Stem / {stem:?}"),
            FxMix { slot, .. } => format!("FX {} / Mix", slot + 1),
            FxOn { slot, .. } => format!("FX {} / On", slot + 1),
            FxPrevious { slot, .. } => format!("FX {} / Previous effect", slot + 1),
            FxNext { slot, .. } => format!("FX {} / Next effect", slot + 1),
            FxSend { .. } => "Echo send".into(),
            BrowseTracks => "Browse tracks (encoder)".into(),
            BrowsePlaylists => "Browse playlists (encoder)".into(),
            TrackPrevious => "Previous track".into(),
            TrackNext => "Next track".into(),
            PlaylistPrevious => "Previous playlist".into(),
            PlaylistNext => "Next playlist".into(),
            LoadSelected { deck } => format!("Load selected track → {deck:?}"),
            LoadFocused => "Load selected track → focused deck".into(),
            Focus { deck } => format!("Focus deck {deck:?}"),
        }
    }
    pub fn label(&self) -> String {
        match self.group() {
            MidiGroup::DeckA | MidiGroup::DeckB => {
                format!("{} / {}", self.group().label(), self.short_label())
            }
            _ => self.short_label(),
        }
    }
}
