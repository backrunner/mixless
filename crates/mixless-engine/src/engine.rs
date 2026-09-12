//! Engine state and public types. Child modules own host operations, command
//! validation, callback rendering, FX parameter transfer and snapshots.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mixless_protocol::{
    Command, CueKind, DeckId, DeckSnapshot, EngineSnapshot, Event, FilterKind, FxParams, FxSlot,
    TrackId, XfCurve,
};
use thiserror::Error;

#[path = "automation.rs"]
mod automation;
#[path = "engine_sync.rs"]
mod sync;

mod commands;
mod fx;
mod host;
mod presentation;
mod render;
mod runtime;
mod state;

#[cfg(test)]
mod tests;

use fx::insert_index;

use crate::decode::{decode_file, AudioBuffer};
use crate::device;
use crate::dsp::{
    db_to_lin, xfader_gains, ChannelFilter, Isolator, MasterLimiter, SeekXf, SmoothValue,
};
use crate::effects::{Effect, EffectKind, EffectParams};
use crate::resample::Resampler;
use crate::stretch::{MusicStretch, StretchParams};

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("device: {0}")]
    Device(#[from] device::DeviceError),
    #[error("decode: {0}")]
    Decode(#[from] crate::decode::DecodeError),
    #[error("no buffer for deck")]
    EmptyDeck,
    #[error("protocol: {0}")]
    Protocol(&'static str),
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub sample_rate: u32,
    pub block_frames: u32,
    pub offline: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            block_frames: 256,
            offline: false,
        }
    }
}

struct DeckRt {
    presentation_step: f64,
    cue_sample: [f32; 2],
    cue_trim: SmoothValue,
    isolator_l: Isolator,
    isolator_r: Isolator,
    filter_l: ChannelFilter,
    filter_r: ChannelFilter,
    seek: SeekXf,
    old_ph: f64,
    last_l: f32,
    last_r: f32,
    position: f64,
    buffer_id: usize,
    source: Option<Arc<AudioBuffer>>,
    touching: bool,
    scratch_speed: f64,
    slip_position: f64,
    amplitude: SmoothValue,
    gain: SmoothValue,
    eq: [SmoothValue; 3],
    step: SmoothValue,
    step_target: f32,
    brake_elapsed: Option<u64>,
    paused_seek: bool,
    playing: bool,
    loop_range: Option<(f64, f64)>,
    slip: bool,
    inserts: [Effect; FxSlot::INSERTS.len()],
    /// Source-frame anchor, beat at the anchor, and local beats per source frame.
    fx_clock: Option<(f64, f64, f64)>,
    resampler: Resampler,
    music: MusicStretch,
    music_params: StretchParams,
    music_mode: bool,
    music_blend: SmoothValue,
    last_music_blend: f32,
    transition_tail: Vec<[f32; 2]>,
    tail_index: usize,
}

pub(crate) struct AudioRt {
    pub(crate) cue_output: Option<device::CueWriter>,
    cue_limiter: MasterLimiter,
    decks: [DeckRt; 2],
    master: SmoothValue,
    cross: [SmoothValue; 2],
    sends: [Effect; 2],
    send_levels: [SmoothValue; 2],
    limiter: MasterLimiter,
    automation: automation::AutomationRt,
}

struct AtomicFx {
    kind: AtomicU32,
    mix: AtomicU32,
    bypass: AtomicBool,
    feedback: AtomicU32,
    beats: AtomicU32,
    rate: AtomicU32,
    depth: AtomicU32,
    drive: AtomicU32,
    decay: AtomicU32,
    size: AtomicU32,
    damping: AtomicU32,
}

struct DeckSlot {
    beat_grid: Mutex<Option<sync::BeatGrid>>,
    buffer: Mutex<Option<Arc<AudioBuffer>>>,
    title: Mutex<Option<String>>,
    artist: Mutex<Option<String>>,
    track_id: AtomicU64,
    playing: AtomicBool,
    playhead: AtomicU64,    // frames * 65536
    rate_micro: AtomicU32,  // rate * 1000000
    pitch_centi: AtomicU32, // (semitones + 24) * 100
    keylock: AtomicBool,
    fader: AtomicU32,      // 0..1000
    gain_milli: AtomicU32, // (db + 96) * 100
    eq_db: [AtomicU32; 3], // (db + 96) * 100
    eq_kill: [AtomicBool; 3],
    filter_milli: AtomicU32, // (amount + 1) * 500
    resonance_milli: AtomicU32,
    resonance_enabled: AtomicBool,
    jog_touch: AtomicBool,
    jog_target: AtomicU64,
    inserts: [AtomicFx; FxSlot::INSERTS.len()],
    send_milli: AtomicU32,
    pfl: AtomicBool,
    vinyl: AtomicBool,
    slip: AtomicBool,
    reverse: AtomicBool,
    roll: AtomicBool,
    roll_division: AtomicU32,
    roll_start: AtomicU64,
    brake: AtomicBool,
    automix_cue: AtomicU64,
    loop_on: AtomicBool,
    loop_sixteenths: AtomicU32,
    frames: AtomicU64,
    src_sr: AtomicU32,
    bpm_milli: AtomicU32, // bpm * 100, 0 = unknown
    loop_length_frames: AtomicU64,
    loop_start: AtomicU64, // source frames * 65536
    seek_pending: AtomicBool,
    seek_from: AtomicU64,
    seek_to: AtomicU64,
    cues: [AtomicU64; 8],      // 0 = empty, else frame+1
    cue_kinds: [AtomicU32; 8], // AUTO=0, IN=1, OUT=2
    temporary_cue: AtomicU64,
    /// Post fader/gain peak level per channel (f32 bits, decayed per block).
    level: [AtomicU32; 2],
    /// Overview waveform computed at load time (host thread only).
    waveform: Mutex<Option<Arc<mixless_protocol::Waveform>>>,
}

pub struct Shared {
    presentation: presentation::PresentationClock,
    pub(crate) audio_failed: AtomicBool,
    pub(crate) cue_failed: AtomicBool,
    cue_device: Mutex<Option<String>>,
    sync_follower: AtomicU32, // 0 = off, 1 = A follows B, 2 = B follows A
    sync_factor: AtomicU32,   // f32 bits: half / same / double tempo
    sync_align: AtomicBool,
    sync_locked: AtomicBool,
    pub sample_rate: AtomicU32,
    pub block_frames: AtomicU32,
    decks: [DeckSlot; 2],
    xfader: AtomicU32, // (pos + 1) * 500
    xf_curve: AtomicU32,
    xf_reverse: AtomicBool,
    master: AtomicU32,
    cue_gain: AtomicU32,
    quantize: AtomicBool,
    xrun: AtomicU64,
    device_name: Mutex<String>,
    last_block: Mutex<[f32; 2]>,
    sends: [AtomicFx; 2],
    automation: automation::AutomationShared,
}

pub struct Engine {
    audio_tx: Option<std::sync::mpsc::Sender<AudioRequest>>,
    audio_apply: Mutex<()>,
    audio_config: Mutex<device::AudioConfig>,
    shared: Arc<Shared>,
    rt: Mutex<AudioRt>,
    retired_buffers: Mutex<Vec<Arc<AudioBuffer>>>,
}

type AudioRequest = (
    device::AudioConfig,
    std::sync::mpsc::Sender<Result<(), device::DeviceError>>,
);

pub struct EventRx;

impl EventRx {
    pub fn try_recv(&mut self) -> Option<Event> {
        None
    }
}

pub use automation::PreparedMix;
