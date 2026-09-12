use serde::{Deserialize, Serialize};

use crate::{CueKind, DeckId, FxSlot, FxState, TrackId, XfCurve, default_fx};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeckSnapshot {
    pub track_id: Option<TrackId>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub playing: bool,
    pub frame: u64,
    pub frames: u64,
    /// Source file sample rate. Playhead `frame` is in this domain, not the device rate.
    pub src_sample_rate: u32,
    pub beat: f32,
    pub bar: f32,
    pub rate: f32,
    pub pitch_semitones: f32,
    #[serde(default = "default_keylock")]
    pub keylock: bool,
    pub sounding_bpm: f32,
    pub eq_db: [f32; 3],
    pub eq_kill: [bool; 3],
    pub filter_amount: f32,
    #[serde(default = "default_filter_resonance")]
    pub filter_resonance: f32,
    #[serde(default = "default_resonance_enabled")]
    pub filter_resonance_enabled: bool,
    pub lp_hz: f32,
    pub hp_hz: f32,
    pub fader: f32,
    pub gain_db: f32,
    pub send: f32,
    pub pfl: bool,
    #[serde(default)]
    pub automix_cue_frame: Option<u64>,
    pub loop_on: bool,
    pub loop_bars: u16,
    #[serde(default = "default_loop_beats")]
    pub loop_beats: f32,
    #[serde(default)]
    pub loop_start_frame: u64,
    #[serde(default)]
    pub loop_end_frame: u64,
    pub vinyl: bool,
    pub slip: bool,
    pub synced: bool,
    #[serde(default)]
    pub sync_master: bool,
    #[serde(default)]
    pub sync_locked: bool,
    #[serde(default)]
    pub grid_ready: bool,
    pub reverse: bool,
    #[serde(default)]
    pub roll: bool,
    #[serde(default = "default_roll_division")]
    pub roll_division: u16,
    #[serde(default)]
    pub brake: bool,
    pub insert: [Option<FxSlot>; FxSlot::INSERTS.len()],
    #[serde(default = "default_fx")]
    pub fx: [FxState; FxSlot::INSERTS.len()],
    pub cues: [Option<u64>; 8],
    #[serde(default = "default_cue_kinds")]
    pub cue_kinds: [CueKind; 8],
    #[serde(default)]
    pub temporary_cue_frame: Option<u64>,
    /// Post fader/gain peak level per channel (0..1+), decayed per block.
    pub level: [f32; 2],
}

impl Default for DeckSnapshot {
    fn default() -> Self {
        Self {
            track_id: None,
            title: None,
            artist: None,
            playing: false,
            frame: 0,
            frames: 0,
            src_sample_rate: 44_100,
            beat: 0.0,
            bar: 0.0,
            rate: 1.0,
            pitch_semitones: 0.0,
            keylock: default_keylock(),
            sounding_bpm: 0.0,
            eq_db: [0.0; 3],
            eq_kill: [false; 3],
            filter_amount: 0.0,
            filter_resonance: default_filter_resonance(),
            filter_resonance_enabled: true,
            lp_hz: 20_000.0,
            hp_hz: 20.0,
            fader: 0.8,
            gain_db: 0.0,
            send: 0.0,
            pfl: false,
            automix_cue_frame: None,
            loop_on: false,
            loop_bars: 1,
            loop_beats: 4.,
            loop_start_frame: 0,
            loop_end_frame: 0,
            vinyl: false,
            slip: false,
            synced: false,
            sync_master: false,
            sync_locked: false,
            grid_ready: false,
            reverse: false,
            roll: false,
            roll_division: default_roll_division(),
            brake: false,
            insert: [None; FxSlot::INSERTS.len()],
            fx: default_fx(),
            cues: [None; 8],
            cue_kinds: default_cue_kinds(),
            temporary_cue_frame: None,
            level: [0.0; 2],
        }
    }
}

fn default_cue_kinds() -> [CueKind; 8] {
    [CueKind::Hot; 8]
}

fn default_roll_division() -> u16 {
    4
}

fn default_filter_resonance() -> f32 {
    0.35
}

fn default_keylock() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub sample_rate: u32,
    pub block_frames: u32,
    pub decks: [DeckSnapshot; 2],
    pub xfader: f32,
    pub xf_curve: XfCurve,
    pub xf_reverse: bool,
    pub master: f32,
    pub cue_gain: f32,
    pub cue_device: Option<String>,
    pub pfl_available: bool,
    pub quantize: bool,
    pub automix_on: bool,
    pub automix_paused: bool,
    #[serde(default)]
    pub automix_progress: f32,
    pub xrun_count: u64,
    pub device_name: String,
}

impl Default for EngineSnapshot {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            block_frames: 256,
            decks: [DeckSnapshot::default(), DeckSnapshot::default()],
            xfader: 0.0,
            xf_curve: XfCurve::EqualPower,
            xf_reverse: false,
            master: 0.8,
            cue_gain: 0.8,
            cue_device: None,
            pfl_available: false,
            quantize: true,
            automix_on: false,
            automix_paused: false,
            automix_progress: 0.0,
            xrun_count: 0,
            device_name: "System Default".into(),
        }
    }
}

impl EngineSnapshot {
    pub fn deck(&self, id: DeckId) -> &DeckSnapshot {
        &self.decks[id.index()]
    }
}

fn default_loop_beats() -> f32 {
    4.
}

fn default_resonance_enabled() -> bool {
    true
}
