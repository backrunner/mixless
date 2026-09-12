use serde::{Deserialize, Serialize};

use crate::{CueKind, TrackId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionLabel {
    Silence,
    Intro,
    Verse,
    BuildUp,
    Drop,
    Break,
    Breakdown,
    Chorus,
    Bridge,
    Outro,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempoSegment {
    pub start_beat: f32,
    pub end_beat: f32,
    pub bpm: f32,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TempoMap {
    pub global_bpm: f32,
    pub meter_num: u8,
    pub meter_den: u8,
    pub segments: Vec<TempoSegment>,
    pub beats: Vec<f32>,
    pub downbeats: Vec<f32>,
}

impl TempoMap {
    pub fn bpm_at_beat(&self, beat: f32) -> f32 {
        self.segments
            .iter()
            .find(|s| beat >= s.start_beat && beat < s.end_beat)
            .map(|s| s.bpm)
            .filter(|b| *b > 0.0)
            .unwrap_or(self.global_bpm)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub start_sec: f32,
    pub end_sec: f32,
    pub label: SectionLabel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarFeature {
    pub bar_index: u32,
    pub start_sec: f32,
    pub end_sec: f32,
    pub rms: f32,
    pub crest: f32,
    pub low_db: f32,
    pub mid_db: f32,
    pub high_db: f32,
    pub chroma: [f32; 12],
    pub chord: Option<String>,
    pub local_key: Option<String>,
    pub onset_density: f32,
    pub kick_salience: f32,
    pub hat_salience: f32,
    pub vocal_presence: f32,
    /// Optional model evidence for human voice. `vocal_presence` also includes
    /// sustained instruments and remains the conservative overlap risk.
    #[serde(default)]
    pub vocal_confidence: Option<f32>,
    pub energy_slope: f32,
    pub section: SectionLabel,
}

/// Measured structural change or a phrase inferred relative to that change.
/// Confidence describes the boundary, not a claim that a section label is exact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhraseBoundary {
    pub time_sec: f32,
    pub confidence: f32,
    /// Zero for a metrical subdivision; positive for measured timbral change.
    pub novelty: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MixRegionKind {
    In,
    Out,
}

/// A source-time mixing window, measured from phrase and bar evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixRegion {
    pub kind: MixRegionKind,
    pub start_sec: f32,
    pub end_sec: f32,
    pub anchor_sec: f32,
    pub confidence: f32,
    pub vocal_risk: f32,
    pub kick: f32,
    pub rms: f32,
    pub camelot: Option<String>,
    pub key_confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackAnalysis {
    pub track_id: TrackId,
    pub duration_sec: f32,
    pub sample_rate: u32,
    pub tempo: TempoMap,
    pub key: Option<String>,
    pub camelot: Option<String>,
    pub key_confidence: f32,
    pub sections: Vec<Section>,
    #[serde(default)]
    pub phrase_boundaries: Vec<PhraseBoundary>,
    #[serde(default)]
    pub mix_regions: Vec<MixRegion>,
    pub bars: Vec<BarFeature>,
    pub waveform_path: Option<String>,
    pub partial: bool,
}

/// Precomputed overview waveform for one track.
///
/// Every column holds 16-bit positive/negative/RMS envelopes plus four band
/// shares (bass, low-mid, high-mid, treble), quantized to 0..=255. Legacy
/// 8-bit envelopes remain available for older consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waveform {
    pub columns: u32,
    pub duration_sec: f32,
    /// Per-column peak amplitude (normalized to the track maximum).
    pub peak: Vec<u8>,
    /// Positive half-wave peak, normalized to the track maximum. Optional in
    /// older cached payloads; the renderer falls back to `peak` when absent.
    #[serde(default)]
    pub peak_pos: Vec<u8>,
    /// Negative half-wave magnitude, normalized to the track maximum.
    #[serde(default)]
    pub peak_neg: Vec<u8>,
    /// Per-column RMS level, using the same normalization as the peaks.
    #[serde(default)]
    pub rms: Vec<u8>,
    /// Per-column low-band energy share.
    pub low: Vec<u8>,
    /// Low-mid share, separated from the bass for red/yellow/green/blue rendering.
    #[serde(default)]
    pub low_mid: Vec<u8>,
    /// Per-column mid-band energy share.
    pub mid: Vec<u8>,
    /// Per-column high-band energy share.
    pub high: Vec<u8>,
    /// 16-bit envelopes for detailed display; older payloads fall back to u8.
    #[serde(default)]
    pub detail_pos: Vec<u16>,
    #[serde(default)]
    pub detail_neg: Vec<u16>,
    #[serde(default)]
    pub detail_rms: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cue {
    pub index: u8,
    pub frame: u64,
    pub kind: CueKind,
    pub user_set: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub path: String,
    /// Cached path to embedded cover art, when the source contains one.
    #[serde(default)]
    pub artwork_path: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: u32,
    pub isrc: Option<String>,
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub camelot: Option<String>,
    pub analyzed: bool,
    pub content_hash: String,
}
