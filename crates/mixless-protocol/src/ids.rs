use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeckId {
    A,
    B,
}

impl DeckId {
    pub fn index(self) -> usize {
        match self {
            DeckId::A => 0,
            DeckId::B => 1,
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(DeckId::A),
            1 => Some(DeckId::B),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlaylistId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BatchId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EqBand {
    Low,
    Mid,
    High,
}

impl EqBand {
    pub fn index(self) -> usize {
        match self {
            EqBand::Low => 0,
            EqBand::Mid => 1,
            EqBand::High => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    Lp,
    Hp,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XfCurve {
    Linear,
    EqualPower,
    Cut,
    Scratch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CueKind {
    Hot,
    In,
    Out,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneId {
    Xfader,
    GainA,
    GainB,
    EqA,
    EqB,
    FilterA,
    FilterB,
    SendA,
    SendB,
    RateA,
    RateB,
    PitchA,
    PitchB,
    InsertA,
    InsertB,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FxSlot {
    Insert0,
    Insert1,
    Insert2,
    Insert3,
    SendEcho,
    SendReverb,
}

impl FxSlot {
    pub const INSERTS: [Self; 4] = [Self::Insert0, Self::Insert1, Self::Insert2, Self::Insert3];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxParams {
    pub mix: f32,
    pub time_beats: Option<f32>,
    pub feedback: Option<f32>,
    pub rate_hz: Option<f32>,
    pub threshold_db: Option<f32>,
    pub kind: Option<String>,
    /// Modulation depth (0..1), used by chorus/flanger/phaser/tremolo/filter.
    pub depth: Option<f32>,
    /// Drive amount (0..1), used by distortion/bitcrusher/noise.
    pub drive: Option<f32>,
    /// Reverb T60 in seconds (0.2..8), independent of room size.
    pub decay_seconds: Option<f32>,
    /// Reverb room size (0..1).
    pub size: Option<f32>,
    /// Reverb high-frequency damping (0..1).
    pub damping: Option<f32>,
}

impl Default for FxParams {
    fn default() -> Self {
        Self {
            mix: 0.0,
            time_beats: None,
            feedback: None,
            rate_hz: None,
            threshold_db: None,
            kind: None,
            depth: None,
            drive: None,
            decay_seconds: None,
            size: None,
            damping: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStage {
    Meta,
    Decode,
    Waveform,
    Tempo,
    Key,
    Bars,
    Structure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Io,
    Decode,
    Device,
    Analysis,
    SpotifyAuth,
    SpotifyApi,
    Plan,
    Protocol,
    Acquire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    Unmatched,
    Local,
    Acquired,
    Suspect,
    Missing,
}
