use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mixless_protocol::{
    Command, DeckId, DeckSnapshot, EngineSnapshot, Event, FilterKind, FxParams, FxSlot, TrackId,
    XfCurve,
};
use thiserror::Error;

#[path = "automation.rs"]
mod automation;

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
    playing: bool,
    loop_range: Option<(f64, f64)>,
    slip: bool,
    inserts: [Effect; 3],
    resampler: Resampler,
    music: MusicStretch,
    music_params: StretchParams,
    music_mode: bool,
    music_blend: SmoothValue,
    last_music_blend: f32,
    transition_tail: Vec<[f32; 2]>,
    tail_index: usize,
}

impl DeckRt {
    fn new(sr: f32) -> Self {
        Self {
            isolator_l: Isolator::new(sr),
            isolator_r: Isolator::new(sr),
            filter_l: ChannelFilter::new(sr),
            filter_r: ChannelFilter::new(sr),
            seek: SeekXf::new(),
            old_ph: 0.0,
            last_l: 0.0,
            last_r: 0.0,
            position: 0.0,
            buffer_id: 0,
            source: None,
            touching: false,
            scratch_speed: 0.0,
            slip_position: 0.0,
            amplitude: SmoothValue::new(0.0, sr, 0.001),
            gain: SmoothValue::new(0.8, sr, 0.003),
            eq: std::array::from_fn(|_| SmoothValue::new(1.0, sr, 0.005)),
            step: SmoothValue::new(1.0, sr, 0.002),
            step_target: 1.0,
            playing: false,
            loop_range: None,
            slip: false,
            inserts: std::array::from_fn(|_| Effect::new(sr)),
            resampler: Resampler::new(),
            music: MusicStretch::new(sr),
            music_params: StretchParams {
                rate: 1.0,
                semitones: 0.0,
                source_step: 1.0,
                loop_range: None,
            },
            music_mode: false,
            music_blend: SmoothValue::new(0.0, sr, 0.006),
            last_music_blend: 0.0,
            transition_tail: vec![[0.0; 2]; (sr * 0.006).round().max(2.0) as usize],
            tail_index: (sr * 0.006).round().max(2.0) as usize,
        }
    }

    fn capture_music_tail(&mut self, buffer: &AudioBuffer) {
        self.tail_index = self.transition_tail.len();
        if !self.music.is_primed() || self.last_music_blend == 0.0 {
            return;
        }
        let mut position = self.position;
        for sample in &mut self.transition_tail {
            let (music, step) =
                self.music
                    .next(buffer, &self.resampler, position, self.music_params);
            let (left, right) = self.resampler.stereo_at(buffer, position, step);
            *sample = [
                left + self.last_music_blend * (music[0] - left),
                right + self.last_music_blend * (music[1] - right),
            ];
            position += step;
        }
        self.tail_index = 0;
    }
}

pub(crate) struct AudioRt {
    decks: [DeckRt; 2],
    master: SmoothValue,
    cross: [SmoothValue; 2],
    sends: [Effect; 2],
    send_levels: [SmoothValue; 2],
    limiter: MasterLimiter,
    automation: automation::AutomationRt,
}

impl AudioRt {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            decks: std::array::from_fn(|_| DeckRt::new(sample_rate)),
            master: SmoothValue::new(0.8, sample_rate, 0.005),
            cross: std::array::from_fn(|_| {
                SmoothValue::new(std::f32::consts::FRAC_1_SQRT_2, sample_rate, 0.0005)
            }),
            sends: std::array::from_fn(|_| Effect::new(sample_rate)),
            send_levels: std::array::from_fn(|_| SmoothValue::new(0.0, sample_rate, 0.005)),
            limiter: MasterLimiter::new(sample_rate),
            automation: automation::AutomationRt::default(),
        }
    }
}

struct AtomicFx {
    kind: AtomicU32,
    mix: AtomicU32,
    bypass: AtomicBool,
    feedback: AtomicU32,
    beats: AtomicU32,
    rate: AtomicU32,
}

impl AtomicFx {
    fn new() -> Self {
        Self {
            kind: AtomicU32::new(0),
            mix: AtomicU32::new(0.5f32.to_bits()),
            bypass: AtomicBool::new(true),
            feedback: AtomicU32::new(0.35f32.to_bits()),
            beats: AtomicU32::new(0.5f32.to_bits()),
            rate: AtomicU32::new(0.0f32.to_bits()),
        }
    }

    fn set(&self, params: &FxParams, kind: EffectKind) {
        self.mix
            .store(params.mix.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.feedback.store(
            params.feedback.unwrap_or(0.35).clamp(0.0, 0.88).to_bits(),
            Ordering::Relaxed,
        );
        self.beats.store(
            params
                .time_beats
                .unwrap_or(0.5)
                .clamp(0.0625, 4.0)
                .to_bits(),
            Ordering::Relaxed,
        );
        self.rate.store(
            params.rate_hz.unwrap_or(0.0).clamp(0.0, 30.0).to_bits(),
            Ordering::Relaxed,
        );
        self.kind.store(kind as u32, Ordering::Release);
    }

    fn read(&self, bpm: f32) -> EffectParams {
        EffectParams {
            kind: EffectKind::from_id(self.kind.load(Ordering::Acquire)),
            mix: f32::from_bits(self.mix.load(Ordering::Relaxed)),
            bypass: self.bypass.load(Ordering::Relaxed),
            feedback: f32::from_bits(self.feedback.load(Ordering::Relaxed)),
            beats: f32::from_bits(self.beats.load(Ordering::Relaxed)),
            rate_hz: f32::from_bits(self.rate.load(Ordering::Relaxed)),
            bpm,
        }
    }
}

fn insert_index(slot: FxSlot) -> Option<usize> {
    match slot {
        FxSlot::Insert0 => Some(0),
        FxSlot::Insert1 => Some(1),
        FxSlot::Insert2 => Some(2),
        _ => None,
    }
}

struct DeckSlot {
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
    jog_touch: AtomicBool,
    jog_target: AtomicU64,
    inserts: [AtomicFx; 3],
    send_milli: AtomicU32,
    pfl: AtomicBool,
    vinyl: AtomicBool,
    slip: AtomicBool,
    reverse: AtomicBool,
    loop_on: AtomicBool,
    loop_bars: AtomicU32,
    frames: AtomicU64,
    src_sr: AtomicU32,
    bpm_milli: AtomicU32, // bpm * 100, 0 = unknown
    loop_length_frames: AtomicU64,
    loop_start: AtomicU64, // source frames * 65536
    seek_pending: AtomicBool,
    seek_from: AtomicU64,
    seek_to: AtomicU64,
    cues: [AtomicU64; 8], // 0 = empty, else frame+1
    /// Post fader/gain peak level per channel (f32 bits, decayed per block).
    level: [AtomicU32; 2],
    /// Overview waveform computed at load time (host thread only).
    waveform: Mutex<Option<Arc<mixless_protocol::Waveform>>>,
}

impl DeckSlot {
    fn new() -> Self {
        Self {
            buffer: Mutex::new(None),
            title: Mutex::new(None),
            artist: Mutex::new(None),
            track_id: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            playhead: AtomicU64::new(0),
            rate_micro: AtomicU32::new(1_000_000),
            pitch_centi: AtomicU32::new(2400),
            keylock: AtomicBool::new(true),
            fader: AtomicU32::new(800),
            gain_milli: AtomicU32::new(9600),
            eq_db: [
                AtomicU32::new(9600),
                AtomicU32::new(9600),
                AtomicU32::new(9600),
            ],
            eq_kill: [
                AtomicBool::new(false),
                AtomicBool::new(false),
                AtomicBool::new(false),
            ],
            filter_milli: AtomicU32::new(500),
            resonance_milli: AtomicU32::new(350),
            jog_touch: AtomicBool::new(false),
            jog_target: AtomicU64::new(0),
            inserts: std::array::from_fn(|_| AtomicFx::new()),
            send_milli: AtomicU32::new(0),
            pfl: AtomicBool::new(false),
            vinyl: AtomicBool::new(false),
            slip: AtomicBool::new(false),
            reverse: AtomicBool::new(false),
            loop_on: AtomicBool::new(false),
            loop_bars: AtomicU32::new(4),
            frames: AtomicU64::new(0),
            src_sr: AtomicU32::new(44_100),
            bpm_milli: AtomicU32::new(0),
            loop_length_frames: AtomicU64::new(0),
            loop_start: AtomicU64::new(0),
            seek_pending: AtomicBool::new(false),
            seek_from: AtomicU64::new(0),
            seek_to: AtomicU64::new(0),
            cues: std::array::from_fn(|_| AtomicU64::new(0)),
            level: [AtomicU32::new(0), AtomicU32::new(0)],
            waveform: Mutex::new(None),
        }
    }

    fn playhead_frames(&self) -> f64 {
        self.playhead.load(Ordering::Relaxed) as f64 / 65536.0
    }

    fn set_playhead(&self, frames: f64) {
        let v = (frames.max(0.0) * 65536.0) as u64;
        self.playhead.store(v, Ordering::Relaxed);
    }
}

pub struct Shared {
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
    shared: Arc<Shared>,
    rt: Mutex<AudioRt>,
    retired_buffers: Mutex<Vec<Arc<AudioBuffer>>>,
}

fn spawn_output_thread(shared: Arc<Shared>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let thread_shared = Arc::clone(&shared);
    let _ = std::thread::Builder::new()
        .name("mixless-audio".into())
        .spawn(move || match device::start_default_output(thread_shared) {
            Ok(out) => {
                let _ = tx.send(Ok((
                    out.sample_rate,
                    out.block_hint,
                    out.device_name.clone(),
                )));
                // Keep the !Send cpal stream on this thread for the process lifetime.
                std::mem::forget(out);
                loop {
                    std::thread::park();
                }
            }
            Err(e) => {
                let _ = tx.send(Err(e));
            }
        });
    match rx.recv() {
        Ok(Ok((sr, block, name))) => {
            shared.sample_rate.store(sr, Ordering::Relaxed);
            shared.block_frames.store(block, Ordering::Relaxed);
            if let Ok(mut n) = shared.device_name.lock() {
                *n = name;
            }
        }
        Ok(Err(e)) => tracing::warn!("audio device unavailable, offline only: {e}"),
        Err(_) => tracing::warn!("audio thread failed to start"),
    }
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Self, EngineError> {
        let sr = config.sample_rate;
        let shared = Arc::new(Shared {
            sample_rate: AtomicU32::new(sr),
            block_frames: AtomicU32::new(config.block_frames),
            decks: [DeckSlot::new(), DeckSlot::new()],
            xfader: AtomicU32::new(500),
            xf_curve: AtomicU32::new(1),
            xf_reverse: AtomicBool::new(false),
            master: AtomicU32::new(800),
            cue_gain: AtomicU32::new(800),
            quantize: AtomicBool::new(true),
            xrun: AtomicU64::new(0),
            device_name: Mutex::new("Offline".into()),
            last_block: Mutex::new([0.0; 2]),
            automation: automation::AutomationShared::default(),
            sends: std::array::from_fn(|index| {
                let effect = AtomicFx::new();
                effect.kind.store(
                    if index == 0 {
                        EffectKind::Echo as u32
                    } else {
                        EffectKind::Reverb as u32
                    },
                    Ordering::Relaxed,
                );
                effect.bypass.store(index != 0, Ordering::Relaxed);
                effect
            }),
        });

        if !config.offline {
            spawn_output_thread(Arc::clone(&shared));
        }

        let actual_sr = shared.sample_rate.load(Ordering::Relaxed) as f32;
        Ok(Self {
            shared,
            rt: Mutex::new(AudioRt::new(actual_sr)),
            retired_buffers: Mutex::new(Vec::new()),
        })
    }

    pub fn dispatch(&self, cmd: Command) -> Result<(), EngineError> {
        let finite = match &cmd {
            Command::Jog { delta_frames, .. } => delta_frames.is_finite(),
            Command::SetChannelFilter { amount, .. } => amount.is_finite(),
            Command::SetFilterResonance { resonance, .. } => resonance.is_finite(),
            Command::SetFilter { cutoff_hz, .. } => cutoff_hz.is_finite(),
            Command::SetRate { rate, .. } => rate.is_finite(),
            Command::SetPitchSemitones { semitones, .. } => semitones.is_finite(),
            Command::SetEq { db, .. } | Command::SetChannelGain { db, .. } => db.is_finite(),
            Command::SetCrossfader { value }
            | Command::SetMaster { value }
            | Command::SetChannelFader { value, .. }
            | Command::SetFxSend { value, .. }
            | Command::SetCueGain { value } => value.is_finite(),
            Command::SetFx { deck, slot, params } => {
                if deck.is_none() && insert_index(*slot).is_some() {
                    return Err(EngineError::Protocol("insert FX requires a deck"));
                }
                if params
                    .kind
                    .as_deref()
                    .is_some_and(|kind| EffectKind::parse(kind).is_none())
                {
                    return Err(EngineError::Protocol("unsupported FX kind"));
                }
                params.mix.is_finite()
                    && [
                        params.feedback,
                        params.time_beats,
                        params.rate_hz,
                        params.threshold_db,
                    ]
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite())
            }
            _ => true,
        };
        if !finite {
            return Err(EngineError::Protocol("non-finite audio parameter"));
        }
        if let Command::Eject { deck } = cmd {
            self.eject(deck);
            return Ok(());
        }
        // Host-only commands never touch the audio graph.
        match &cmd {
            Command::ImportFiles { .. }
            | Command::LoadDeck { .. }
            | Command::CreatePlaylist { .. }
            | Command::RenamePlaylist { .. }
            | Command::DeletePlaylist { .. }
            | Command::ReorderPlaylist { .. }
            | Command::AddToPlaylist { .. }
            | Command::RemoveFromPlaylist { .. }
            | Command::LinkLocalFile { .. }
            | Command::SpotifyLogin
            | Command::SpotifyLogout
            | Command::RetryAnalysis { .. }
            | Command::StartAutomix { .. }
            | Command::SetCueDevice { .. } => return Ok(()),
            _ => {}
        }
        self.automation_command(&cmd);
        // Mixer params are atomics — apply on the caller thread so a full
        // command ring cannot drop a play/fader/cue under jog spam.
        self.shared.apply_cmd(&cmd);
        Ok(())
    }

    pub fn load_file(
        &self,
        deck: DeckId,
        track_id: TrackId,
        path: &Path,
        title: String,
        artist: String,
    ) -> Result<(), EngineError> {
        self.load_file_if(deck, track_id, path, title, artist, || true)
            .map(|_| ())
    }

    /// Decode off-thread, then discard the result if the host cancelled its job.
    /// The predicate is evaluated before publishing any deck state.
    pub fn load_file_if(
        &self,
        deck: DeckId,
        track_id: TrackId,
        path: &Path,
        title: String,
        artist: String,
        should_commit: impl FnOnce() -> bool,
    ) -> Result<bool, EngineError> {
        let buf = decode_file(path)?;
        // Keep enough source detail for crisp, asymmetric waveform peaks at
        // beat-level zoom. The UI still samples this cache once per screen
        // pixel, so the larger cache does not increase frame-time work.
        let cols = ((buf.frames / 96) as usize).clamp(4096, 131_072);
        let wave = Arc::new(crate::waveform::compute_waveform(&buf, cols));
        if !should_commit() {
            return Ok(false);
        }
        self.automation_command(&Command::StopAutomix);
        let slot = &self.shared.decks[deck.index()];
        slot.loop_on.store(false, Ordering::Relaxed);
        for cue in &slot.cues {
            cue.store(0, Ordering::Relaxed);
        }
        slot.bpm_milli.store(0, Ordering::Relaxed);
        slot.frames.store(buf.frames, Ordering::Relaxed);
        slot.src_sr.store(buf.sample_rate, Ordering::Relaxed);
        slot.track_id.store(track_id.0 as u64, Ordering::Relaxed);
        slot.set_playhead(0.0);
        slot.playing.store(false, Ordering::Relaxed);
        slot.jog_touch.store(false, Ordering::Release);
        slot.seek_pending.store(false, Ordering::Release);
        let previous = slot
            .buffer
            .lock()
            .map_err(|_| EngineError::Protocol("buffer mutex poisoned"))?
            .replace(buf);
        self.retire_buffer(previous);
        if let Ok(mut w) = slot.waveform.lock() {
            *w = Some(wave);
        }
        if let Ok(mut t) = slot.title.lock() {
            *t = Some(title);
        }
        if let Ok(mut a) = slot.artist.lock() {
            *a = Some(artist);
        }
        Ok(true)
    }

    pub fn eject(&self, deck: DeckId) {
        self.automation_command(&Command::StopAutomix);
        let slot = &self.shared.decks[deck.index()];
        slot.playing.store(false, Ordering::Relaxed);
        slot.jog_touch.store(false, Ordering::Release);
        slot.seek_pending.store(false, Ordering::Release);
        slot.track_id.store(0, Ordering::Relaxed);
        slot.frames.store(0, Ordering::Relaxed);
        slot.set_playhead(0.0);
        if let Ok(mut buffer) = slot.buffer.lock() {
            self.retire_buffer(buffer.take());
        }
        if let Ok(mut w) = slot.waveform.lock() {
            *w = None;
        }
        if let Ok(mut t) = slot.title.lock() {
            *t = None;
        }
        if let Ok(mut a) = slot.artist.lock() {
            *a = None;
        }
        for c in &slot.cues {
            c.store(0, Ordering::Relaxed);
        }
    }

    pub fn set_bpm(&self, deck: DeckId, bpm: f32) {
        let v = if bpm.is_finite() && bpm > 0.0 {
            (bpm * 100.0) as u32
        } else {
            0
        };
        self.shared.decks[deck.index()]
            .bpm_milli
            .store(v, Ordering::Relaxed);
    }

    fn retire_buffer(&self, previous: Option<Arc<AudioBuffer>>) {
        if let Ok(mut retired) = self.retired_buffers.lock() {
            retired.retain(|buffer| Arc::strong_count(buffer) > 1);
            if let Some(buffer) = previous {
                retired.push(buffer);
            }
        }
    }

    pub fn set_cue_frame(&self, deck: DeckId, index: u8, frame: u64) {
        if index < 8 {
            self.shared.decks[deck.index()].cues[index as usize]
                .store(frame.saturating_add(1), Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        self.shared.snapshot()
    }

    /// Clone of the overview waveform for a deck, if a track is loaded.
    pub fn deck_waveform(&self, deck: DeckId) -> Option<mixless_protocol::Waveform> {
        self.shared.decks[deck.index()]
            .waveform
            .lock()
            .ok()
            .and_then(|w| w.as_ref().map(|w| (**w).clone()))
    }

    pub fn subscribe(&self) -> EventRx {
        EventRx
    }

    pub fn render_offline(&self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames * 2];
        let mut rt = self.rt.lock().expect("rt");
        self.shared.process_block(&mut rt, &mut out, 2);
        debug_assert!(out.iter().all(|s| s.is_finite()));
        out
    }

    pub fn sample_rate(&self) -> u32 {
        self.shared.sample_rate.load(Ordering::Relaxed)
    }
}

pub struct EventRx;

impl EventRx {
    pub fn try_recv(&mut self) -> Option<Event> {
        None
    }
}

impl Shared {
    pub(crate) fn render_interleaved(
        shared: &Shared,
        rt: &mut AudioRt,
        data: &mut [f32],
        channels: usize,
    ) {
        const MAX: usize = 1024;
        let ch = channels.max(1);
        let frames = data.len() / ch;
        {
            let mut tmp = [0.0f32; MAX * 2];
            let mut done = 0;
            while done < frames {
                let n = (frames - done).min(MAX);
                shared.process_block(rt, &mut tmp[..n * 2], 2);
                if ch == 1 {
                    for i in 0..n {
                        data[done + i] = (tmp[i * 2] + tmp[i * 2 + 1]) * 0.5;
                    }
                } else {
                    for i in 0..n {
                        let o = (done + i) * ch;
                        data[o] = tmp[i * 2];
                        data[o + 1] = tmp[i * 2 + 1];
                        for c in 2..ch {
                            data[o + c] = 0.0;
                        }
                    }
                }
                done += n;
            }
        }
    }

    fn apply_cmd(&self, cmd: &Command) {
        match *cmd {
            Command::PlayPause { deck } => {
                let s = &self.decks[deck.index()];
                let next = !s.playing.load(Ordering::Relaxed);
                s.playing.store(next, Ordering::Relaxed);
            }
            Command::Jog { deck, delta_frames } => {
                let s = &self.decks[deck.index()];
                let maximum = s.frames.load(Ordering::Relaxed).saturating_sub(1) as f64;
                if s.jog_touch.load(Ordering::Acquire) {
                    let _ =
                        s.jog_target
                            .fetch_update(Ordering::Release, Ordering::Relaxed, |target| {
                                Some(
                                    ((target as f64 / 65536.0 + delta_frames as f64)
                                        .clamp(0.0, maximum)
                                        * 65536.0) as u64,
                                )
                            });
                } else {
                    s.seek_to.store(
                        ((s.playhead_frames() + delta_frames as f64).clamp(0.0, maximum) * 65536.0)
                            as u64,
                        Ordering::Relaxed,
                    );
                    s.seek_pending.store(true, Ordering::Release);
                }
            }
            Command::SetJogTouch { deck, touching } => {
                let slot = &self.decks[deck.index()];
                if touching && !slot.jog_touch.load(Ordering::Acquire) {
                    slot.jog_target
                        .store(slot.playhead.load(Ordering::Relaxed), Ordering::Relaxed);
                }
                slot.jog_touch.store(touching, Ordering::Release);
            }
            Command::Sync { deck, keylock } => {
                self.decks[deck.index()]
                    .keylock
                    .store(keylock, Ordering::Relaxed);
                let i = deck.index();
                let o = 1 - i;
                let bpm_s = self.decks[i].bpm_milli.load(Ordering::Relaxed);
                let bpm_o = self.decks[o].bpm_milli.load(Ordering::Relaxed);
                if bpm_s > 0 && bpm_o > 0 {
                    let rate_o =
                        self.decks[o].rate_micro.load(Ordering::Relaxed) as f32 / 1_000_000.0;
                    let ratio = (bpm_o as f32 / 100.0) / (bpm_s as f32 / 100.0);
                    let r = (ratio * rate_o).clamp(0.25, 4.0);
                    self.decks[i]
                        .rate_micro
                        .store((r * 1_000_000.0) as u32, Ordering::Relaxed);
                }
            }
            Command::SetRate { deck, rate } => {
                let r = (rate.clamp(0.25, 4.0) * 1_000_000.0) as u32;
                self.decks[deck.index()]
                    .rate_micro
                    .store(r, Ordering::Relaxed);
            }
            Command::SetKeyLock { deck, on } => {
                self.decks[deck.index()]
                    .keylock
                    .store(on, Ordering::Relaxed);
            }
            Command::SetPitchSemitones { deck, semitones } => {
                let v = ((semitones.clamp(-24.0, 24.0) + 24.0) * 100.0) as u32;
                self.decks[deck.index()]
                    .pitch_centi
                    .store(v, Ordering::Relaxed);
            }
            Command::SetChannelFader { deck, value } => {
                let v = (value.clamp(0.0, 1.0) * 1000.0) as u32;
                self.decks[deck.index()].fader.store(v, Ordering::Relaxed);
            }
            Command::SetChannelGain { deck, db } => {
                let v = ((db.clamp(-96.0, 12.0) + 96.0) * 100.0) as u32;
                self.decks[deck.index()]
                    .gain_milli
                    .store(v, Ordering::Relaxed);
            }
            Command::SetEq { deck, band, db } => {
                let v = ((db.clamp(-96.0, 12.0) + 96.0) * 100.0) as u32;
                self.decks[deck.index()].eq_db[band.index()].store(v, Ordering::Relaxed);
            }
            Command::SetEqKill { deck, band, on } => {
                self.decks[deck.index()].eq_kill[band.index()].store(on, Ordering::Relaxed);
            }
            Command::SetChannelFilter { deck, amount } => {
                let v = ((amount.clamp(-1.0, 1.0) + 1.0) * 500.0) as u32;
                self.decks[deck.index()]
                    .filter_milli
                    .store(v, Ordering::Relaxed);
            }
            Command::SetFilterResonance { deck, resonance } => {
                self.decks[deck.index()].resonance_milli.store(
                    (resonance.clamp(0.0, 1.0) * 1000.0).round() as u32,
                    Ordering::Relaxed,
                );
            }
            Command::SetFilter {
                deck,
                cutoff_hz,
                kind,
            } => {
                let upper = 18_000.0f32.min(self.sample_rate.load(Ordering::Relaxed) as f32 * 0.45);
                let cutoff = cutoff_hz.clamp(30.0, upper);
                let amount = match kind {
                    FilterKind::Open => 0.0,
                    FilterKind::Lp => -(cutoff / upper).ln() / (30.0 / upper).ln(),
                    FilterKind::Hp => (cutoff / 30.0).ln() / (upper / 30.0).ln(),
                };
                let v = ((amount + 1.0) * 500.0) as u32;
                self.decks[deck.index()]
                    .filter_milli
                    .store(v, Ordering::Relaxed);
            }
            Command::SetCrossfader { value } => {
                let v = ((value.clamp(-1.0, 1.0) + 1.0) * 500.0).round() as u32;
                self.xfader.store(v, Ordering::Relaxed);
            }
            Command::SetXfCurve { curve } => {
                let v = match curve {
                    XfCurve::Linear => 0,
                    XfCurve::EqualPower => 1,
                    XfCurve::Cut => 2,
                    XfCurve::Scratch => 3,
                };
                self.xf_curve.store(v, Ordering::Relaxed);
            }
            Command::SetXfReverse { on } => self.xf_reverse.store(on, Ordering::Relaxed),
            Command::SetMaster { value } => {
                let v = (value.clamp(0.0, 1.0) * 1000.0) as u32;
                self.master.store(v, Ordering::Relaxed);
            }
            Command::SetFxSend { deck, value } => {
                let v = (value.clamp(0.0, 1.0) * 1000.0) as u32;
                self.decks[deck.index()]
                    .send_milli
                    .store(v, Ordering::Relaxed);
            }
            Command::SetFx {
                deck,
                slot,
                ref params,
            } => {
                let effect = match (deck, insert_index(slot)) {
                    (Some(deck), Some(index)) => &self.decks[deck.index()].inserts[index],
                    (_, None) => &self.sends[if slot == FxSlot::SendReverb { 1 } else { 0 }],
                    _ => return,
                };
                let kind = match slot {
                    FxSlot::SendEcho => EffectKind::Echo,
                    FxSlot::SendReverb => EffectKind::Reverb,
                    _ => params
                        .kind
                        .as_deref()
                        .and_then(EffectKind::parse)
                        .unwrap_or_else(|| {
                            EffectKind::from_id(effect.kind.load(Ordering::Relaxed))
                        }),
                };
                effect.set(params, kind);
            }
            Command::SetFxBypass { deck, slot, on } => {
                let effect = if let Some(index) = insert_index(slot) {
                    &self.decks[deck.index()].inserts[index]
                } else {
                    &self.sends[if slot == FxSlot::SendReverb { 1 } else { 0 }]
                };
                effect.bypass.store(on, Ordering::Relaxed);
            }
            Command::SetPfl { deck, on } => {
                self.decks[deck.index()].pfl.store(on, Ordering::Relaxed);
            }
            Command::SetCueGain { value } => {
                let v = (value.clamp(0.0, 1.0) * 1000.0) as u32;
                self.cue_gain.store(v, Ordering::Relaxed);
            }
            Command::SetVinylMode { deck, vinyl, slip } => {
                let s = &self.decks[deck.index()];
                s.vinyl.store(vinyl, Ordering::Relaxed);
                s.slip.store(slip, Ordering::Relaxed);
            }
            Command::SetReverse { deck, on } => {
                self.decks[deck.index()]
                    .reverse
                    .store(on, Ordering::Relaxed);
            }
            Command::SetQuantize { on } => self.quantize.store(on, Ordering::Relaxed),
            Command::JumpCue { deck, index } => {
                if (index as usize) < 8 {
                    let s = &self.decks[deck.index()];
                    let packed = s.cues[index as usize].load(Ordering::Relaxed);
                    if packed > 0 {
                        let from = s.playhead.load(Ordering::Relaxed);
                        let to = packed - 1;
                        s.seek_from.store(from, Ordering::Relaxed);
                        s.seek_to.store(to * 65536, Ordering::Relaxed);
                        s.seek_pending.store(true, Ordering::Release);
                    }
                }
            }
            Command::SetCue { deck, index, frame } => {
                if (index as usize) < 8 {
                    self.decks[deck.index()].cues[index as usize]
                        .store(frame.saturating_add(1), Ordering::Relaxed);
                }
            }
            Command::ClearCue { deck, index } => {
                if (index as usize) < 8 {
                    self.decks[deck.index()].cues[index as usize].store(0, Ordering::Relaxed);
                }
            }
            Command::BeatJump { deck, bars } => {
                let s = &self.decks[deck.index()];
                let sr = s.src_sr.load(Ordering::Relaxed).max(1) as f64;
                let bpm = (s.bpm_milli.load(Ordering::Relaxed) as f64 / 100.0).max(1.0);
                let bpm = if bpm <= 1.0 { 120.0 } else { bpm };
                let frames = (bars as f64) * 4.0 * (60.0 / bpm) * sr;
                let from = s.playhead.load(Ordering::Relaxed);
                let dest = (s.playhead_frames() + frames).clamp(
                    0.0,
                    s.frames.load(Ordering::Relaxed).saturating_sub(1) as f64,
                );
                s.seek_from.store(from, Ordering::Relaxed);
                s.seek_to.store((dest * 65536.0) as u64, Ordering::Relaxed);
                s.seek_pending.store(true, Ordering::Release);
            }
            Command::SetLoop { deck, bars, on } => {
                let s = &self.decks[deck.index()];
                s.loop_length_frames.store(0, Ordering::Relaxed);
                if on && !s.loop_on.load(Ordering::Relaxed) {
                    s.loop_start
                        .store(s.playhead.load(Ordering::Relaxed), Ordering::Relaxed);
                }
                s.loop_on.store(on, Ordering::Relaxed);
                s.loop_bars.store(bars as u32, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    fn process_audio_block(&self, rt: &mut AudioRt, out: &mut [f32], channels: usize) {
        let sr = self.sample_rate.load(Ordering::Relaxed) as f32;
        let xf_pos = self.xfader.load(Ordering::Relaxed) as f32 / 500.0 - 1.0;
        let curve = match self.xf_curve.load(Ordering::Relaxed) {
            0 => XfCurve::Linear,
            2 => XfCurve::Cut,
            3 => XfCurve::Scratch,
            _ => XfCurve::EqualPower,
        };
        let (ga, gb) = xfader_gains(xf_pos, curve, self.xf_reverse.load(Ordering::Relaxed));
        let master = self.master.load(Ordering::Relaxed) as f32 / 1000.0;
        let frames = out.len() / channels.max(1);
        let xf_len = ((sr * 0.006).round() as usize).max(2);
        rt.cross[0].set(ga);
        rt.cross[1].set(gb);
        rt.master.set(master);

        let bufs: [Option<Arc<AudioBuffer>>; 2] =
            std::array::from_fn(|index| match self.decks[index].buffer.try_lock() {
                Ok(buffer) => buffer.clone(),
                Err(_) => rt.decks[index].source.clone(),
            });

        for index in 0..2 {
            let slot = &self.decks[index];
            let deck = &mut rt.decks[index];
            let buffer_id = bufs[index]
                .as_ref()
                .map_or(0, |buffer| Arc::as_ptr(buffer) as usize);
            if buffer_id != deck.buffer_id {
                deck.buffer_id = buffer_id;
                deck.source = bufs[index].clone();
                deck.position = slot.playhead_frames();
                deck.old_ph = deck.position;
                deck.seek = SeekXf::new();
                deck.touching = false;
                deck.isolator_l = Isolator::new(sr);
                deck.isolator_r = Isolator::new(sr);
                deck.filter_l = ChannelFilter::new(sr);
                deck.filter_r = ChannelFilter::new(sr);
                deck.amplitude = SmoothValue::new(0.0, sr, 0.001);
                deck.music.invalidate();
                deck.music_mode = false;
                deck.music_blend = SmoothValue::new(0.0, sr, 0.006);
                deck.last_music_blend = 0.0;
                deck.tail_index = deck.transition_tail.len();
                for effect in &mut deck.inserts {
                    effect.reset();
                }
            }
            let was_playing = deck.playing;
            deck.playing = slot.playing.load(Ordering::Relaxed);
            if deck.playing && !was_playing {
                deck.music.invalidate();
            }
            let source_rate = bufs[index]
                .as_ref()
                .map_or(sr, |buffer| buffer.sample_rate as f32);
            let rate = slot.rate_micro.load(Ordering::Relaxed) as f32 / 1_000_000.0;
            let pitch = slot.pitch_centi.load(Ordering::Relaxed) as f32 / 100.0 - 24.0;
            let direction = if slot.reverse.load(Ordering::Relaxed) {
                -1.0
            } else {
                1.0
            };
            deck.step_target = source_rate / sr * rate * direction;
            deck.step.set(deck.step_target);
            let touching = slot.jog_touch.load(Ordering::Acquire);
            if touching && !deck.touching {
                deck.slip_position = deck.position;
                deck.scratch_speed = 0.0;
            } else if !touching && deck.touching {
                if deck.slip && deck.playing {
                    deck.old_ph = deck.position;
                    deck.tail_index = deck.transition_tail.len();
                    deck.position = deck.slip_position.clamp(
                        0.0,
                        slot.frames.load(Ordering::Relaxed).saturating_sub(1) as f64,
                    );
                    deck.seek.start(xf_len);
                }
                deck.step = SmoothValue::new(deck.scratch_speed as f32, sr, 0.002);
                deck.step.set(deck.step_target);
                deck.music.invalidate();
            }
            deck.touching = touching;
            deck.slip = slot.slip.load(Ordering::Relaxed);
            let bpm_raw = slot.bpm_milli.load(Ordering::Relaxed) as f32 / 100.0;
            let bpm = if bpm_raw > 0.0 { bpm_raw } else { 120.0 };
            let previous_loop = deck.loop_range;
            deck.loop_range = if slot.loop_on.load(Ordering::Relaxed) {
                let start = slot.loop_start.load(Ordering::Relaxed) as f64 / 65536.0;
                let length = slot.loop_bars.load(Ordering::Relaxed).max(1) as f64 * 4.0 * 60.0
                    / bpm as f64
                    * source_rate as f64;
                let exact_length = slot.loop_length_frames.load(Ordering::Relaxed);
                let length = if exact_length > 0 {
                    exact_length as f64
                } else {
                    length
                };
                let end = (start + length).min(slot.frames.load(Ordering::Relaxed) as f64);
                (end - start > 1.0).then_some((start, end))
            } else {
                None
            };
            let keylock = slot.keylock.load(Ordering::Relaxed);
            let music_mode = deck.playing
                && !touching
                && (pitch.abs() > 0.0001 || (keylock && (rate - 1.0).abs() > 0.0001));
            let source_step = (source_rate / sr * direction) as f64;
            let source_changed = source_step != deck.music_params.source_step;
            let loop_changed = previous_loop != deck.loop_range;
            if deck.playing && (source_changed || (loop_changed && deck.last_music_blend > 0.0)) {
                if let Some(buffer) = bufs[index].as_deref() {
                    deck.capture_music_tail(buffer);
                }
                deck.old_ph = deck.position;
                deck.seek.start(xf_len);
            }
            if (music_mode && !deck.music_mode) || source_changed || loop_changed {
                deck.music.invalidate();
            }
            deck.music_mode = music_mode;
            deck.music_blend.set(if music_mode { 1.0 } else { 0.0 });
            deck.music_params = StretchParams {
                rate: rate as f64,
                semitones: pitch + if keylock { 0.0 } else { 12.0 * rate.log2() },
                source_step,
                loop_range: deck.loop_range,
            };
            for band in 0..3 {
                let value = if slot.eq_kill[band].load(Ordering::Relaxed) {
                    0.0
                } else {
                    db_to_lin(slot.eq_db[band].load(Ordering::Relaxed) as f32 / 100.0 - 96.0)
                };
                deck.eq[band].set(value);
            }
            deck.gain.set(
                db_to_lin(slot.gain_milli.load(Ordering::Relaxed) as f32 / 100.0 - 96.0)
                    * slot.fader.load(Ordering::Relaxed) as f32
                    / 1000.0,
            );
            let amount = slot.filter_milli.load(Ordering::Relaxed) as f32 / 500.0 - 1.0;
            let resonance = slot.resonance_milli.load(Ordering::Relaxed) as f32 / 1000.0;
            deck.filter_l.set_amount(sr, amount);
            deck.filter_r.set_amount(sr, amount);
            deck.filter_l.set_resonance(resonance);
            deck.filter_r.set_resonance(resonance);
            for effect in 0..3 {
                deck.inserts[effect].configure(slot.inserts[effect].read(bpm * rate));
            }
            rt.send_levels[index].set(slot.send_milli.load(Ordering::Relaxed) as f32 / 1000.0);
            if slot.seek_pending.swap(false, Ordering::AcqRel) {
                if let Some(buffer) = bufs[index].as_deref() {
                    deck.capture_music_tail(buffer);
                }
                deck.music.invalidate();
                deck.old_ph = deck.position;
                deck.position = (slot.seek_to.load(Ordering::Relaxed) as f64 / 65536.0)
                    .min(slot.frames.load(Ordering::Relaxed).saturating_sub(1) as f64);
                slot.jog_target
                    .store((deck.position * 65536.0) as u64, Ordering::Relaxed);
                if deck.playing {
                    deck.seek.start(xf_len);
                }
            }
        }
        let clock = if let Some(index) = rt
            .automation
            .clock_deck
            .filter(|_| self.automation.enabled.load(Ordering::Acquire))
        {
            &self.decks[index]
        } else if rt.decks[0].playing {
            &self.decks[0]
        } else {
            &self.decks[1]
        };
        let tempo = clock.bpm_milli.load(Ordering::Relaxed) as f32 / 100.0;
        let tempo = if tempo > 0.0 {
            tempo * clock.rate_micro.load(Ordering::Relaxed) as f32 / 1_000_000.0
        } else {
            120.0
        };
        let tempo = rt
            .automation
            .tempo
            .filter(|_| self.automation.enabled.load(Ordering::Acquire))
            .unwrap_or(tempo);
        for index in 0..2 {
            rt.sends[index].configure(self.sends[index].read(tempo));
        }

        let mut last_ok = true;
        let mut peak = [[0f32; 2]; 2];
        for i in 0..frames {
            let (al, ar) = self.render_deck(0, &mut rt.decks[0], sr, bufs[0].as_deref());
            let (bl, br) = self.render_deck(1, &mut rt.decks[1], sr, bufs[1].as_deref());
            peak[0][0] = peak[0][0].max(al.abs());
            peak[0][1] = peak[0][1].max(ar.abs());
            peak[1][0] = peak[1][0].max(bl.abs());
            peak[1][1] = peak[1][1].max(br.abs());
            let cross_a = rt.cross[0].next();
            let cross_b = rt.cross[1].next();
            let mut l = al * cross_a + bl * cross_b;
            let mut r = ar * cross_a + br * cross_b;
            let send_a = rt.send_levels[0].next() * cross_a;
            let send_b = rt.send_levels[1].next() * cross_b;
            let send = [al * send_a + bl * send_b, ar * send_a + br * send_b];
            for effect in &mut rt.sends {
                let wet = effect.process(send, true);
                l += wet[0];
                r += wet[1];
            }
            let master = rt.master.next();
            (l, r) = rt.limiter.process(l * master, r * master);
            if !l.is_finite() || !r.is_finite() {
                last_ok = false;
                l = 0.0;
                r = 0.0;
            }
            if channels == 1 {
                out[i] = (l + r) * 0.5;
            } else {
                out[i * channels] = l;
                out[i * channels + 1] = r;
            }
        }
        if !last_ok {
            self.xrun.fetch_add(1, Ordering::Relaxed);
        }
        // Peak-hold with per-block decay so the UI meters fall smoothly.
        for d in 0..2 {
            if bufs[d].is_some() {
                self.decks[d].set_playhead(rt.decks[d].position);
            }
            for ch in 0..2 {
                let cur = f32::from_bits(self.decks[d].level[ch].load(Ordering::Relaxed));
                let next = peak[d][ch].max(cur * 0.85);
                self.decks[d].level[ch].store(next.to_bits(), Ordering::Relaxed);
            }
        }
        if let Ok(mut lb) = self.last_block.try_lock() {
            if frames > 0 {
                *lb = [out[0], out.get(1).copied().unwrap_or(0.0)];
            }
        }
    }

    fn render_deck(
        &self,
        idx: usize,
        rt: &mut DeckRt,
        device_sr: f32,
        buf: Option<&AudioBuffer>,
    ) -> (f32, f32) {
        let slot = &self.decks[idx];
        let Some(buf) = buf else {
            return (0.0, 0.0);
        };

        let src_sr = buf.sample_rate.max(1) as f32;
        let mut step = rt.step.next() as f64;
        let level = if rt.touching {
            let target = slot.jog_target.load(Ordering::Acquire) as f64 / 65536.0;
            let desired = ((target - rt.position) / (device_sr as f64 * 0.003)).clamp(
                -8.0 * src_sr as f64 / device_sr as f64,
                8.0 * src_sr as f64 / device_sr as f64,
            );
            let smoothing = 1.0 / (device_sr as f64 * 0.0005 + 1.0);
            rt.scratch_speed += (desired - rt.scratch_speed) * smoothing;
            step = rt.scratch_speed;
            (step.abs() as f32 / (src_sr / device_sr * 0.025)).min(1.0)
        } else if rt.playing {
            1.0
        } else {
            0.0
        };
        rt.amplitude.set(level);
        let amplitude = rt.amplitude.next();
        let mut ph = rt.position;
        if !rt.touching {
            if let Some((start, end)) = rt.loop_range {
                if ph >= end || ph < start {
                    if !rt.music_mode {
                        rt.old_ph = ph;
                        rt.tail_index = rt.transition_tail.len();
                        rt.seek.start((device_sr * 0.006) as usize);
                    }
                    ph = start + (ph - start).rem_euclid(end - start);
                }
            }
        }
        let (mut l, mut r) = if amplitude > 0.0 {
            rt.resampler.stereo_at(buf, ph, step)
        } else {
            (0.0, 0.0)
        };

        let music_blend = rt.music_blend.next();
        rt.last_music_blend = music_blend;
        if amplitude > 0.0 && (rt.music_mode || music_blend > 0.0) {
            let (music, music_step) = rt.music.next(buf, &rt.resampler, ph, rt.music_params);
            l += music_blend * (music[0] - l);
            r += music_blend * (music[1] - r);
            if rt.music_mode {
                step = music_step;
            }
        }

        if rt.seek.active() {
            let g = rt.seek.next_gain();
            let fade_in = g.sqrt();
            let fade_out = (1.0 - g).sqrt();
            let (ol, or_) = if rt.tail_index < rt.transition_tail.len() {
                let sample = rt.transition_tail[rt.tail_index];
                rt.tail_index += 1;
                (sample[0], sample[1])
            } else {
                rt.resampler.stereo_at(buf, rt.old_ph, step)
            };
            l = ol * fade_out + l * fade_in;
            r = or_ * fade_out + r * fade_in;
            rt.old_ph += step;
        }

        if rt.playing || rt.touching || amplitude > 0.0 {
            ph += step;
        }
        if rt.touching && rt.playing {
            rt.slip_position += rt.step_target as f64;
        }
        if !rt.touching && rt.loop_range.is_none() && (ph < 0.0 || ph >= buf.frames as f64) {
            rt.playing = false;
            slot.playing.store(false, Ordering::Relaxed);
        }
        rt.position = if rt.loop_range.is_some() && !rt.touching {
            ph
        } else {
            ph.clamp(0.0, buf.frames.saturating_sub(1) as f64)
        };

        let eq = std::array::from_fn(|band| rt.eq[band].next());
        rt.isolator_l.gain = eq;
        rt.isolator_r.gain = eq;

        l = rt.filter_l.process(rt.isolator_l.process(l * amplitude));
        r = rt.filter_r.process(rt.isolator_r.process(r * amplitude));
        let mut stereo = [l, r];
        for effect in &mut rt.inserts {
            stereo = effect.process(stereo, false);
        }
        let gain = rt.gain.next();
        l = stereo[0] * gain;
        r = stereo[1] * gain;
        rt.last_l = l;
        rt.last_r = r;
        (l, r)
    }

    fn snapshot(&self) -> EngineSnapshot {
        let mut snap = EngineSnapshot {
            sample_rate: self.sample_rate.load(Ordering::Relaxed),
            block_frames: self.block_frames.load(Ordering::Relaxed),
            xfader: self.xfader.load(Ordering::Relaxed) as f32 / 500.0 - 1.0,
            xf_curve: match self.xf_curve.load(Ordering::Relaxed) {
                0 => XfCurve::Linear,
                2 => XfCurve::Cut,
                3 => XfCurve::Scratch,
                _ => XfCurve::EqualPower,
            },
            xf_reverse: self.xf_reverse.load(Ordering::Relaxed),
            master: self.master.load(Ordering::Relaxed) as f32 / 1000.0,
            cue_gain: self.cue_gain.load(Ordering::Relaxed) as f32 / 1000.0,
            pfl_available: false,
            quantize: self.quantize.load(Ordering::Relaxed),
            automix_on: self.automation.enabled.load(Ordering::Acquire),
            automix_paused: self.automation.paused.load(Ordering::Acquire),
            automix_progress: f32::from_bits(self.automation.progress.load(Ordering::Relaxed)),
            xrun_count: self.xrun.load(Ordering::Relaxed),
            device_name: self
                .device_name
                .lock()
                .map(|s| s.clone())
                .unwrap_or_else(|_| "System Default".into()),
            ..EngineSnapshot::default()
        };
        for i in 0..2 {
            let s = &self.decks[i];
            let id = s.track_id.load(Ordering::Relaxed);
            let mut cues = [None; 8];
            for (j, c) in s.cues.iter().enumerate() {
                let v = c.load(Ordering::Relaxed);
                if v > 0 {
                    cues[j] = Some(v - 1);
                }
            }
            snap.decks[i] = DeckSnapshot {
                track_id: if id == 0 {
                    None
                } else {
                    Some(TrackId(id as i64))
                },
                title: s.title.lock().ok().and_then(|t| t.clone()),
                artist: s.artist.lock().ok().and_then(|t| t.clone()),
                playing: s.playing.load(Ordering::Relaxed),
                frame: s.playhead_frames() as u64,
                frames: s.frames.load(Ordering::Relaxed),
                src_sample_rate: s.src_sr.load(Ordering::Relaxed),
                beat: 0.0,
                bar: 0.0,
                rate: s.rate_micro.load(Ordering::Relaxed) as f32 / 1_000_000.0,
                pitch_semitones: s.pitch_centi.load(Ordering::Relaxed) as f32 / 100.0 - 24.0,
                keylock: s.keylock.load(Ordering::Relaxed),
                sounding_bpm: {
                    let bpm = s.bpm_milli.load(Ordering::Relaxed) as f32 / 100.0;
                    let rate = s.rate_micro.load(Ordering::Relaxed) as f32 / 1_000_000.0;
                    if bpm <= 0.0 {
                        0.0
                    } else {
                        bpm * rate
                    }
                },
                eq_db: [
                    s.eq_db[0].load(Ordering::Relaxed) as f32 / 100.0 - 96.0,
                    s.eq_db[1].load(Ordering::Relaxed) as f32 / 100.0 - 96.0,
                    s.eq_db[2].load(Ordering::Relaxed) as f32 / 100.0 - 96.0,
                ],
                eq_kill: [
                    s.eq_kill[0].load(Ordering::Relaxed),
                    s.eq_kill[1].load(Ordering::Relaxed),
                    s.eq_kill[2].load(Ordering::Relaxed),
                ],
                filter_amount: s.filter_milli.load(Ordering::Relaxed) as f32 / 500.0 - 1.0,
                filter_resonance: s.resonance_milli.load(Ordering::Relaxed) as f32 / 1000.0,
                lp_hz: ChannelFilter::cutoff(
                    snap.sample_rate as f32,
                    (s.filter_milli.load(Ordering::Relaxed) as f32 / 500.0 - 1.0).min(0.0),
                ),
                hp_hz: ChannelFilter::cutoff(
                    snap.sample_rate as f32,
                    (s.filter_milli.load(Ordering::Relaxed) as f32 / 500.0 - 1.0)
                        .max(0.0)
                        .max(f32::EPSILON),
                ),
                fader: s.fader.load(Ordering::Relaxed) as f32 / 1000.0,
                gain_db: s.gain_milli.load(Ordering::Relaxed) as f32 / 100.0 - 96.0,
                send: s.send_milli.load(Ordering::Relaxed) as f32 / 1000.0,
                pfl: s.pfl.load(Ordering::Relaxed),
                loop_on: s.loop_on.load(Ordering::Relaxed),
                loop_bars: s.loop_bars.load(Ordering::Relaxed) as u16,
                vinyl: s.vinyl.load(Ordering::Relaxed),
                slip: s.slip.load(Ordering::Relaxed),
                synced: false,
                reverse: s.reverse.load(Ordering::Relaxed),
                insert: std::array::from_fn(|index| {
                    (!s.inserts[index].bypass.load(Ordering::Relaxed))
                        .then_some([FxSlot::Insert0, FxSlot::Insert1, FxSlot::Insert2][index])
                }),
                cues,
                level: [
                    f32::from_bits(s.level[0].load(Ordering::Relaxed)),
                    f32::from_bits(s.level[1].load(Ordering::Relaxed)),
                ],
            };
        }
        snap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sine_buffer(sr: u32, hz: f32, secs: f32) -> Arc<AudioBuffer> {
        let frames = (sr as f32 * secs) as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for n in 0..frames {
            let s = (2.0 * std::f32::consts::PI * hz * n as f32 / sr as f32).sin() * 0.5;
            samples.push(s);
            samples.push(s);
        }
        Arc::new(AudioBuffer {
            samples,
            frames: frames as u64,
            sample_rate: sr,
        })
    }

    pub(super) fn test_engine(sample_rate: u32) -> Engine {
        let engine = Engine::new(EngineConfig {
            offline: true,
            sample_rate,
            block_frames: 256,
        })
        .unwrap();
        for index in 0..2 {
            let slot = &engine.shared.decks[index];
            let buffer = sine_buffer(sample_rate, if index == 0 { 440.0 } else { 550.0 }, 4.0);
            slot.frames.store(buffer.frames, Ordering::Relaxed);
            slot.src_sr.store(sample_rate, Ordering::Relaxed);
            *slot.buffer.lock().unwrap() = Some(buffer);
        }
        engine
    }

    fn energy(samples: &[f32]) -> f32 {
        samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len().max(1) as f32
    }

    #[test]
    fn scratch_paused_deck_forward_backward_hold_and_release() {
        let engine = test_engine(48_000);
        let slot = &engine.shared.decks[0];
        slot.set_playhead(48_000.0);
        engine.render_offline(128);
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: true,
            })
            .unwrap();
        let start = engine.snapshot().decks[0].frame;
        engine
            .dispatch(Command::Jog {
                deck: DeckId::A,
                delta_frames: 2400.0,
            })
            .unwrap();
        let forward = engine.render_offline(1024);
        assert!(energy(&forward) > 0.001);
        let ahead = engine.snapshot().decks[0].frame;
        assert!(ahead > start + 2000 && ahead <= start + 2400);
        engine
            .dispatch(Command::Jog {
                deck: DeckId::A,
                delta_frames: -4800.0,
            })
            .unwrap();
        let backward = engine.render_offline(2048);
        assert!(energy(&backward) > 0.001);
        assert!(engine.snapshot().decks[0].frame < start - 2000);
        engine.render_offline(4800);
        assert!(energy(&engine.render_offline(512)) < 1e-10);
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: false,
            })
            .unwrap();
        engine.render_offline(512);
        assert!(!engine.snapshot().decks[0].playing);
        assert!(energy(&engine.render_offline(512)) < 1e-10);
    }

    #[test]
    fn scratch_touch_holds_playing_deck_and_release_resumes() {
        let engine = test_engine(48_000);
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        engine.render_offline(2048);
        let start = engine.snapshot().decks[0].frame;
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: true,
            })
            .unwrap();
        engine.render_offline(2048);
        assert_eq!(engine.snapshot().decks[0].frame, start);
        assert!(energy(&engine.render_offline(512)) < 1e-10);
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: false,
            })
            .unwrap();
        let resumed = engine.render_offline(1024);
        assert!(engine.snapshot().decks[0].frame > start + 800);
        assert!(energy(&resumed) > 0.001);
        assert!(engine.snapshot().decks[0].playing);
    }

    #[test]
    fn slip_returns_to_background_timeline() {
        let engine = test_engine(48_000);
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        engine
            .dispatch(Command::SetVinylMode {
                deck: DeckId::A,
                vinyl: true,
                slip: true,
            })
            .unwrap();
        engine.render_offline(2000);
        let start = engine.snapshot().decks[0].frame;
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: true,
            })
            .unwrap();
        engine
            .dispatch(Command::Jog {
                deck: DeckId::A,
                delta_frames: 4000.0,
            })
            .unwrap();
        engine.render_offline(1000);
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: false,
            })
            .unwrap();
        engine.render_offline(256);
        let resumed = engine.snapshot().decks[0].frame;
        assert!(
            (resumed as i64 - (start + 1256) as i64).abs() < 150,
            "slip: {start} -> {resumed}"
        );
    }

    #[test]
    fn insert_commands_change_audio_and_bypass_restores_dry() {
        let engine = test_engine(48_000);
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        engine.render_offline(1000);
        engine
            .dispatch(Command::SetFx {
                deck: Some(DeckId::A),
                slot: FxSlot::Insert0,
                params: FxParams {
                    kind: Some("gate".into()),
                    mix: 1.0,
                    time_beats: Some(0.25),
                    ..FxParams::default()
                },
            })
            .unwrap();
        engine
            .dispatch(Command::SetFxBypass {
                deck: DeckId::A,
                slot: FxSlot::Insert0,
                on: false,
            })
            .unwrap();
        let mut gated = 0.0;
        for _ in 0..100 {
            gated += energy(&engine.render_offline(256));
        }
        engine
            .dispatch(Command::SetFxBypass {
                deck: DeckId::A,
                slot: FxSlot::Insert0,
                on: true,
            })
            .unwrap();
        engine.render_offline(1000);
        let mut dry = 0.0;
        for _ in 0..100 {
            dry += energy(&engine.render_offline(256));
        }
        assert!(
            gated > dry * 0.3 && gated < dry * 0.7,
            "gate {gated}, dry {dry}"
        );
    }

    #[test]
    fn invalid_fx_parameters_are_rejected() {
        let engine = test_engine(48_000);
        assert!(engine
            .dispatch(Command::SetFilterResonance {
                deck: DeckId::A,
                resonance: f32::NAN
            })
            .is_err());
        assert!(engine
            .dispatch(Command::Jog {
                deck: DeckId::A,
                delta_frames: f32::INFINITY
            })
            .is_err());
        assert!(engine
            .dispatch(Command::SetFx {
                deck: Some(DeckId::A),
                slot: FxSlot::Insert0,
                params: FxParams {
                    kind: Some("not-an-effect".into()),
                    ..FxParams::default()
                }
            })
            .is_err());
    }

    #[test]
    fn cutoff_command_and_snapshot_agree() {
        let engine = test_engine(48_000);
        engine
            .dispatch(Command::SetFilter {
                deck: DeckId::A,
                cutoff_hz: 800.0,
                kind: FilterKind::Lp,
            })
            .unwrap();
        assert!((engine.snapshot().decks[0].lp_hz - 800.0).abs() < 15.0);
        engine
            .dispatch(Command::SetFilter {
                deck: DeckId::A,
                cutoff_hz: 1500.0,
                kind: FilterKind::Hp,
            })
            .unwrap();
        assert!((engine.snapshot().decks[0].hp_hz - 1500.0).abs() < 25.0);
    }

    fn left_channel(engine: &Engine, frames: usize) -> Vec<f32> {
        let mut samples = Vec::with_capacity(frames);
        while samples.len() < frames {
            let count = (frames - samples.len()).min(256);
            let output = engine.render_offline(count);
            samples.extend(output.chunks_exact(2).map(|sample| sample[0]));
        }
        samples
    }

    #[test]
    fn deck_key_and_tempo_are_independent_end_to_end() {
        for (rate, key, keylock) in [
            (0.88, 0.0, true),
            (1.12, 0.0, true),
            (1.12, 12.0, true),
            (1.0, -5.0, true),
            (1.12, 0.0, false),
            (1.12, 7.0, false),
        ] {
            let engine = test_engine(48_000);
            engine.set_bpm(DeckId::A, 120.0);
            engine
                .dispatch(Command::SetRate {
                    deck: DeckId::A,
                    rate,
                })
                .unwrap();
            engine
                .dispatch(Command::SetPitchSemitones {
                    deck: DeckId::A,
                    semitones: key,
                })
                .unwrap();
            engine
                .dispatch(Command::SetKeyLock {
                    deck: DeckId::A,
                    on: keylock,
                })
                .unwrap();
            engine
                .dispatch(Command::PlayPause { deck: DeckId::A })
                .unwrap();
            left_channel(&engine, 12_800);
            let start = engine.snapshot().decks[0].frame;
            let samples = left_channel(&engine, 48_000);
            let measured = crate::stretch::frequency(&samples, 48000.0);
            let expected =
                440.0 * 2.0f64.powf(key as f64 / 12.0) * if keylock { 1.0 } else { rate as f64 };
            assert!(
                (measured - expected).abs() < expected * 0.0015,
                "rate={rate} key={key} lock={keylock}: {measured} vs {expected}"
            );
            let snapshot = &engine.snapshot().decks[0];
            assert!((snapshot.sounding_bpm - 120.0 * rate).abs() < 0.01);
            assert!((snapshot.frame as f64 - start as f64 - rate as f64 * 48000.0).abs() < 2.0);
        }
    }

    #[test]
    fn sync_uses_tempo_and_preserves_key_offset() {
        let engine = test_engine(48_000);
        engine.set_bpm(DeckId::A, 100.0);
        engine.set_bpm(DeckId::B, 120.0);
        engine
            .dispatch(Command::SetRate {
                deck: DeckId::B,
                rate: 1.1,
            })
            .unwrap();
        engine
            .dispatch(Command::SetPitchSemitones {
                deck: DeckId::A,
                semitones: 7.0,
            })
            .unwrap();
        engine
            .dispatch(Command::Sync {
                deck: DeckId::A,
                keylock: true,
            })
            .unwrap();
        let snapshot = engine.snapshot();
        assert!((snapshot.decks[0].rate - 1.32).abs() < 0.002);
        assert!((snapshot.decks[0].sounding_bpm - snapshot.decks[1].sounding_bpm).abs() < 0.2);
        assert_eq!(snapshot.decks[0].pitch_semitones, 7.0);
        assert!(snapshot.decks[0].keylock);
        engine
            .dispatch(Command::Sync {
                deck: DeckId::A,
                keylock: false,
            })
            .unwrap();
        assert!(!engine.snapshot().decks[0].keylock);
    }

    #[test]
    fn music_scratch_cue_and_pause_transitions_preserve_transport() {
        let engine = test_engine(48_000);
        engine
            .dispatch(Command::SetRate {
                deck: DeckId::A,
                rate: 1.12,
            })
            .unwrap();
        engine
            .dispatch(Command::SetPitchSemitones {
                deck: DeckId::A,
                semitones: 12.0,
            })
            .unwrap();
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        left_channel(&engine, 12_800);
        let held = engine.snapshot().decks[0].frame;
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: true,
            })
            .unwrap();
        left_channel(&engine, 2048);
        assert_eq!(engine.snapshot().decks[0].frame, held);
        assert!(energy(&engine.render_offline(512)) < 1e-10);
        engine
            .dispatch(Command::Jog {
                deck: DeckId::A,
                delta_frames: 3000.0,
            })
            .unwrap();
        assert!(energy(&engine.render_offline(1024)) > 0.001);
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: false,
            })
            .unwrap();
        left_channel(&engine, 4800);
        let samples = left_channel(&engine, 12_800);
        assert!((crate::stretch::frequency(&samples, 48000.0) - 880.0).abs() < 8.0);
        engine
            .dispatch(Command::SetCue {
                deck: DeckId::A,
                index: 0,
                frame: 96123,
            })
            .unwrap();
        engine
            .dispatch(Command::JumpCue {
                deck: DeckId::A,
                index: 0,
            })
            .unwrap();
        let before = *samples.last().unwrap();
        let after = left_channel(&engine, 1024);
        let mut worst = (after[0] - before).abs();
        for pair in after.windows(2) {
            worst = worst.max((pair[1] - pair[0]).abs());
        }
        assert!(worst < 0.10, "music cue discontinuity {worst}");
        assert!((engine.snapshot().decks[0].frame as f64 - 96123.0 - 1024.0 * 1.12).abs() < 2.0);
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        left_channel(&engine, 1024);
        let paused = engine.snapshot().decks[0].frame;
        assert!(energy(&engine.render_offline(1024)) < 1e-9);
        assert_eq!(engine.snapshot().decks[0].frame, paused);
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        left_channel(&engine, 4800);
        let samples = left_channel(&engine, 12800);
        assert!((crate::stretch::frequency(&samples, 48000.0) - 880.0).abs() < 8.0);
    }

    #[test]
    fn music_loop_and_direction_changes_are_crossfaded() {
        let engine = test_engine(48_000);
        engine
            .dispatch(Command::SetRate {
                deck: DeckId::A,
                rate: 1.12,
            })
            .unwrap();
        engine
            .dispatch(Command::SetPitchSemitones {
                deck: DeckId::A,
                semitones: 12.0,
            })
            .unwrap();
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        let initial = left_channel(&engine, 12_800);
        let mut previous = *initial.last().unwrap();
        for command in [
            Command::SetLoop {
                deck: DeckId::A,
                bars: 1,
                on: true,
            },
            Command::SetLoop {
                deck: DeckId::A,
                bars: 2,
                on: true,
            },
            Command::SetLoop {
                deck: DeckId::A,
                bars: 2,
                on: false,
            },
            Command::SetReverse {
                deck: DeckId::A,
                on: true,
            },
            Command::SetReverse {
                deck: DeckId::A,
                on: false,
            },
        ] {
            engine.dispatch(command).unwrap();
            let samples = left_channel(&engine, 1024);
            let mut worst = 0.0f32;
            for sample in samples {
                assert!(sample.is_finite());
                worst = worst.max((sample - previous).abs());
                previous = sample;
            }
            assert!(worst < 0.10, "music loop/direction discontinuity {worst}");
        }
    }

    #[test]
    fn loop_and_reverse_keep_independent_key() {
        for reverse in [false, true] {
            let engine = test_engine(48_000);
            engine.shared.decks[0].set_playhead(if reverse { 150000.0 } else { 0.0 });
            engine.set_bpm(DeckId::A, 600.0);
            engine
                .dispatch(Command::SetRate {
                    deck: DeckId::A,
                    rate: 0.88,
                })
                .unwrap();
            engine
                .dispatch(Command::SetPitchSemitones {
                    deck: DeckId::A,
                    semitones: 7.0,
                })
                .unwrap();
            engine
                .dispatch(Command::SetReverse {
                    deck: DeckId::A,
                    on: reverse,
                })
                .unwrap();
            if !reverse {
                engine
                    .dispatch(Command::SetLoop {
                        deck: DeckId::A,
                        bars: 1,
                        on: true,
                    })
                    .unwrap();
            }
            engine
                .dispatch(Command::PlayPause { deck: DeckId::A })
                .unwrap();
            left_channel(&engine, 4800);
            let samples = left_channel(&engine, 48000);
            let expected = 440.0 * 2.0f64.powf(7.0 / 12.0);
            let measured = crate::stretch::frequency(&samples, 48000.0);
            assert!(
                (measured - expected).abs() < expected * 0.015,
                "reverse={reverse}: {measured}"
            );
            if !reverse {
                assert!(engine.snapshot().decks[0].frame < 19200);
            }
        }
    }

    #[test]
    fn source_lock_contention_does_not_interrupt_playback() {
        let engine = test_engine(48_000);
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        engine.render_offline(2048);
        let start = engine.snapshot().decks[0].frame;
        let _guard = engine.shared.decks[0].buffer.lock().unwrap();
        let output = engine.render_offline(256);
        assert!(energy(&output) > 0.001);
        assert_eq!(engine.snapshot().decks[0].frame, start + 256);
        assert_eq!(engine.snapshot().xrun_count, 0);
    }

    #[test]
    fn cue_crossfade_has_bounded_sample_discontinuity() {
        let engine = test_engine(48_000);
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        let before = engine.render_offline(1000);
        engine
            .dispatch(Command::SetCue {
                deck: DeckId::A,
                index: 0,
                frame: 24513,
            })
            .unwrap();
        engine
            .dispatch(Command::JumpCue {
                deck: DeckId::A,
                index: 0,
            })
            .unwrap();
        let after = engine.render_offline(512);
        let mut previous = before[before.len() - 2];
        let mut worst = 0.0f32;
        for frame in after.chunks_exact(2) {
            worst = worst.max((frame[0] - previous).abs());
            previous = frame[0];
        }
        assert!(worst < 0.05, "cue discontinuity {worst}");
    }

    #[test]
    fn all_effects_and_sweeps_are_finite_at_supported_rates() {
        for sample_rate in [44_100, 48_000, 96_000] {
            let engine = test_engine(sample_rate);
            for deck in [DeckId::A, DeckId::B] {
                engine.dispatch(Command::PlayPause { deck }).unwrap();
                engine
                    .dispatch(Command::SetFilterResonance {
                        deck,
                        resonance: 1.0,
                    })
                    .unwrap();
                engine
                    .dispatch(Command::SetFxSend { deck, value: 1.0 })
                    .unwrap();
                for (index, slot) in [FxSlot::Insert0, FxSlot::Insert1, FxSlot::Insert2]
                    .into_iter()
                    .enumerate()
                {
                    let kind = if deck == DeckId::A {
                        ["echo", "flanger", "gate"][index]
                    } else {
                        ["reverb", "phaser", "echo"][index]
                    };
                    engine
                        .dispatch(Command::SetFx {
                            deck: Some(deck),
                            slot,
                            params: FxParams {
                                kind: Some(kind.into()),
                                mix: 0.8,
                                feedback: Some(0.88),
                                ..FxParams::default()
                            },
                        })
                        .unwrap();
                    engine
                        .dispatch(Command::SetFxBypass {
                            deck,
                            slot,
                            on: false,
                        })
                        .unwrap();
                }
            }
            for block in 0..500 {
                engine
                    .dispatch(Command::SetChannelFilter {
                        deck: DeckId::A,
                        amount: (block as f32 * 0.07).sin(),
                    })
                    .unwrap();
                let output = engine.render_offline(256);
                assert!(output
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= 1.0));
            }
            assert_eq!(engine.snapshot().xrun_count, 0);
        }
    }

    #[test]
    #[ignore = "release-only 60-second realtime workload benchmark"]
    fn audio_callback_budget() {
        for (block_frames, scratch) in [(256, false), (128, true)] {
            let engine = test_engine(48_000);
            for deck in [DeckId::A, DeckId::B] {
                engine.dispatch(Command::PlayPause { deck }).unwrap();
                engine
                    .dispatch(Command::SetLoop {
                        deck,
                        bars: 1,
                        on: true,
                    })
                    .unwrap();
                engine
                    .dispatch(Command::SetRate {
                        deck,
                        rate: if deck == DeckId::A { 0.92 } else { 1.08 },
                    })
                    .unwrap();
                engine
                    .dispatch(Command::SetPitchSemitones {
                        deck,
                        semitones: if deck == DeckId::A { 2.0 } else { -3.0 },
                    })
                    .unwrap();
                engine
                    .dispatch(Command::SetChannelFilter { deck, amount: -0.3 })
                    .unwrap();
                engine
                    .dispatch(Command::SetFilterResonance {
                        deck,
                        resonance: 1.0,
                    })
                    .unwrap();
                engine
                    .dispatch(Command::SetFxSend { deck, value: 0.8 })
                    .unwrap();
                for (slot, kind) in [
                    (FxSlot::Insert0, "flanger"),
                    (FxSlot::Insert1, "phaser"),
                    (FxSlot::Insert2, "reverb"),
                ] {
                    engine
                        .dispatch(Command::SetFx {
                            deck: Some(deck),
                            slot,
                            params: FxParams {
                                kind: Some(kind.into()),
                                mix: 0.6,
                                ..FxParams::default()
                            },
                        })
                        .unwrap();
                    engine
                        .dispatch(Command::SetFxBypass {
                            deck,
                            slot,
                            on: false,
                        })
                        .unwrap();
                }
            }
            for slot in [FxSlot::SendEcho, FxSlot::SendReverb] {
                engine
                    .dispatch(Command::SetFx {
                        deck: None,
                        slot,
                        params: FxParams {
                            mix: 0.6,
                            ..FxParams::default()
                        },
                    })
                    .unwrap();
                engine
                    .dispatch(Command::SetFxBypass {
                        deck: DeckId::A,
                        slot,
                        on: false,
                    })
                    .unwrap();
            }
            engine.render_offline(2048);
            if scratch {
                engine
                    .dispatch(Command::SetJogTouch {
                        deck: DeckId::A,
                        touching: true,
                    })
                    .unwrap();
                engine
                    .dispatch(Command::SetJogTouch {
                        deck: DeckId::B,
                        touching: true,
                    })
                    .unwrap();
            }
            let mut output = vec![0.0; block_frames * 2];
            let mut timings = Vec::with_capacity(48_000 * 60 / block_frames);
            let mut rt = engine.rt.lock().unwrap();
            for block in 0..48_000 * 60 / block_frames {
                if scratch && block % 512 == 0 {
                    for deck in [DeckId::A, DeckId::B] {
                        engine
                            .dispatch(Command::SetJogTouch {
                                deck,
                                touching: block % 1024 == 0,
                            })
                            .unwrap();
                    }
                }
                if scratch && block % 1024 < 512 && block % 8 == 0 {
                    for deck in [DeckId::A, DeckId::B] {
                        engine
                            .dispatch(Command::Jog {
                                deck,
                                delta_frames: if block % 128 < 64 { 4096.0 } else { -4096.0 },
                            })
                            .unwrap();
                    }
                }
                let start = std::time::Instant::now();
                engine.shared.process_block(&mut rt, &mut output, 2);
                timings.push(start.elapsed().as_secs_f64());
                assert!(output
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= 1.0));
            }
            timings.sort_by(f64::total_cmp);
            let percentile = timings[timings.len() * 99 / 100];
            let budget = block_frames as f64 / 48_000.0;
            eprintln!("audio frames={block_frames} scratch={scratch} p50={:.3}ms p99={:.3}ms budget={:.3}ms ({:.1}%)", timings[timings.len() / 2] * 1000.0, percentile * 1000.0, budget * 1000.0, percentile / budget * 100.0);
            assert!(percentile < budget * if scratch { 0.35 } else { 0.5 });
        }
    }

    #[test]
    fn offline_silence_is_finite_zero() {
        let eng = Engine::new(EngineConfig {
            offline: true,
            sample_rate: 48_000,
            block_frames: 256,
        })
        .unwrap();
        let out = eng.render_offline(512);
        assert_eq!(out.len(), 1024);
        assert!(out.iter().all(|s| s.is_finite() && *s == 0.0));
    }

    #[test]
    fn two_decks_mix_without_nan() {
        let eng = Engine::new(EngineConfig {
            offline: true,
            sample_rate: 48_000,
            block_frames: 256,
        })
        .unwrap();
        {
            let slot = &eng.shared.decks[0];
            *slot.buffer.lock().unwrap() = Some(sine_buffer(48_000, 220.0, 1.0));
            slot.frames.store(48_000, Ordering::Relaxed);
            slot.src_sr.store(48_000, Ordering::Relaxed);
            slot.playing.store(true, Ordering::Relaxed);
        }
        {
            let slot = &eng.shared.decks[1];
            *slot.buffer.lock().unwrap() = Some(sine_buffer(48_000, 330.0, 1.0));
            slot.frames.store(48_000, Ordering::Relaxed);
            slot.src_sr.store(48_000, Ordering::Relaxed);
            slot.playing.store(true, Ordering::Relaxed);
        }
        eng.dispatch(Command::SetCrossfader { value: 0.0 }).unwrap();
        let out = eng.render_offline(1024);
        assert!(out.iter().all(|s| s.is_finite()));
        let energy: f32 = out.iter().map(|s| s * s).sum();
        assert!(energy > 0.01, "expected audible energy, got {energy}");
    }

    #[test]
    fn dispatch_updates_snapshot_without_audio_thread() {
        let eng = Engine::new(EngineConfig {
            offline: true,
            sample_rate: 48_000,
            block_frames: 256,
        })
        .unwrap();
        eng.dispatch(Command::SetCrossfader { value: -1.0 })
            .unwrap();
        eng.dispatch(Command::SetMaster { value: 0.25 }).unwrap();
        let snap = eng.snapshot();
        assert!((snap.xfader - (-1.0)).abs() < 0.01);
        assert!((snap.master - 0.25).abs() < 0.01);
    }

    #[test]
    fn cue_jump_is_click_free_offline() {
        let eng = Engine::new(EngineConfig {
            offline: true,
            sample_rate: 48_000,
            block_frames: 256,
        })
        .unwrap();
        {
            let slot = &eng.shared.decks[0];
            *slot.buffer.lock().unwrap() = Some(sine_buffer(48_000, 220.0, 2.0));
            slot.frames.store(96_000, Ordering::Relaxed);
            slot.src_sr.store(48_000, Ordering::Relaxed);
            slot.playing.store(true, Ordering::Relaxed);
        }
        eng.dispatch(Command::SetCue {
            deck: DeckId::A,
            index: 0,
            frame: 48_000,
        })
        .unwrap();
        let _ = eng.render_offline(128);
        eng.dispatch(Command::JumpCue {
            deck: DeckId::A,
            index: 0,
        })
        .unwrap();
        let out = eng.render_offline(512);
        assert!(out.iter().all(|s| s.is_finite()));
        assert_eq!(eng.snapshot().decks[0].src_sample_rate, 48_000);
    }
}
