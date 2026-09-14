//! Deck control defaults, playhead access and UI snapshot construction.

use super::*;

impl DeckSlot {
    pub(super) fn new() -> Self {
        Self {
            stems: Mutex::new(None),
            stem_gain: std::array::from_fn(|_| AtomicU32::new(1000)),
            beat_grid: Mutex::new(None),
            buffer: Mutex::new(None),
            title: Mutex::new(None),
            artist: Mutex::new(None),
            track_id: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            playhead: AtomicU64::new(0),
            rate_micro: AtomicU32::new(1_000_000),
            pitch_centi: AtomicU32::new(2400),
            keylock: AtomicBool::new(true),
            fader: AtomicU32::new(1000),
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
            resonance_enabled: AtomicBool::new(true),
            jog_touch: AtomicBool::new(false),
            jog_target: AtomicU64::new(0),
            inserts: mixless_protocol::default_fx().map(|state| {
                let fx = AtomicFx::new();
                fx.set(&state.params(), state.kind);
                fx
            }),
            send_milli: AtomicU32::new(0),
            pfl: AtomicBool::new(false),
            vinyl: AtomicBool::new(false),
            slip: AtomicBool::new(false),
            reverse: AtomicBool::new(false),
            roll: AtomicBool::new(false),
            roll_division: AtomicU32::new(4),
            roll_start: AtomicU64::new(0),
            brake: AtomicBool::new(false),
            automix_cue: AtomicU64::new(0),
            loop_on: AtomicBool::new(false),
            loop_sixteenths: AtomicU32::new(64),
            frames: AtomicU64::new(0),
            src_sr: AtomicU32::new(44_100),
            bpm_milli: AtomicU32::new(0),
            loop_length_frames: AtomicU64::new(0),
            loop_start: AtomicU64::new(0),
            seek_pending: AtomicBool::new(false),
            seek_from: AtomicU64::new(0),
            seek_to: AtomicU64::new(0),
            cues: std::array::from_fn(|_| AtomicU64::new(0)),
            cue_kinds: std::array::from_fn(|_| AtomicU32::new(0)),
            temporary_cue: AtomicU64::new(0),
            preview_cue: AtomicU64::new(0),
            level: [AtomicU32::new(0), AtomicU32::new(0)],
            waveform: Mutex::new(None),
        }
    }

    pub(super) fn seek_cue(&self, frame: u64) {
        let frame = frame.min(self.frames.load(Ordering::Relaxed).saturating_sub(1));
        let start = self.loop_start.load(Ordering::Relaxed) / 65536;
        let end = start + self.loop_length_frames.load(Ordering::Relaxed);
        if frame < start || frame >= end {
            self.loop_on.store(false, Ordering::Relaxed);
        }
        self.roll.store(false, Ordering::Relaxed);
        self.brake.store(false, Ordering::Relaxed);
        self.seek_from
            .store(self.playhead.load(Ordering::Relaxed), Ordering::Relaxed);
        self.seek_to.store(frame * 65536, Ordering::Relaxed);
        self.seek_pending.store(true, Ordering::Release);
    }

    pub(super) fn playhead_frames(&self) -> f64 {
        self.playhead.load(Ordering::Relaxed) as f64 / 65536.0
    }

    pub(super) fn set_playhead(&self, frames: f64) {
        let v = (frames.max(0.0) * 65536.0) as u64;
        self.playhead.store(v, Ordering::Relaxed);
    }
}

impl Shared {
    pub(super) fn snapshot(&self) -> EngineSnapshot {
        let cue_device = self.cue_device.lock().expect("cue device").clone();
        let pfl_available = cue_device.is_some()
            && !self.audio_failed.load(Ordering::Relaxed)
            && !self.cue_failed.load(Ordering::Relaxed);
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
            master_level: std::array::from_fn(|i| {
                f32::from_bits(self.master_level[i].load(Ordering::Relaxed))
            }),
            cue_gain: self.cue_gain.load(Ordering::Relaxed) as f32 / 1000.0,
            cue_device,
            pfl_available,
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
            let (beat, bar, bpm, grid_ready) = self.grid_snapshot(i);
            let follower = self.sync_follower.load(Ordering::Acquire);
            snap.decks[i] = DeckSnapshot {
                stems_ready: s.stems_ready(),
                stem_gain: std::array::from_fn(|j| {
                    s.stem_gain[j].load(Ordering::Relaxed) as f32 / 1000.
                }),
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
                beat,
                bar,
                rate: s.rate_micro.load(Ordering::Relaxed) as f32 / 1_000_000.0,
                pitch_semitones: s.pitch_centi.load(Ordering::Relaxed) as f32 / 100.0 - 24.0,
                keylock: s.keylock.load(Ordering::Relaxed),
                sounding_bpm: {
                    let bpm = bpm.unwrap_or(s.bpm_milli.load(Ordering::Relaxed) as f32 / 100.0);
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
                filter_resonance_enabled: s.resonance_enabled.load(Ordering::Relaxed),
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
                automix_cue_frame: s.automix_cue.load(Ordering::Relaxed).checked_sub(1),
                loop_on: s.loop_on.load(Ordering::Relaxed),
                loop_bars: (s.loop_sixteenths.load(Ordering::Relaxed) / 64) as u16,
                loop_beats: s.loop_sixteenths.load(Ordering::Relaxed) as f32 / 16.,
                loop_start_frame: s.loop_start.load(Ordering::Relaxed) / 65536,
                loop_end_frame: s.loop_start.load(Ordering::Relaxed) / 65536
                    + s.loop_length_frames.load(Ordering::Relaxed),
                vinyl: s.vinyl.load(Ordering::Relaxed),
                slip: s.slip.load(Ordering::Relaxed),
                synced: follower == i as u32 + 1,
                sync_master: follower != 0 && follower != i as u32 + 1,
                sync_locked: follower == i as u32 + 1 && self.sync_locked.load(Ordering::Relaxed),
                grid_ready,
                reverse: s.reverse.load(Ordering::Relaxed),
                roll: s.roll.load(Ordering::Relaxed),
                roll_division: s.roll_division.load(Ordering::Relaxed) as u16,
                brake: s.brake.load(Ordering::Relaxed),
                insert: std::array::from_fn(|index| {
                    (!s.inserts[index].bypass.load(Ordering::Relaxed))
                        .then_some(FxSlot::INSERTS[index])
                }),
                fx: std::array::from_fn(|index| s.inserts[index].snapshot()),
                cues,
                cue_kinds: std::array::from_fn(|j| match s.cue_kinds[j].load(Ordering::Relaxed) {
                    1 => CueKind::In,
                    2 => CueKind::Out,
                    _ => CueKind::Hot,
                }),
                temporary_cue_frame: s.temporary_cue.load(Ordering::Relaxed).checked_sub(1),
                cue_previewing: s.preview_cue.load(Ordering::Relaxed) > 0
                    && s.playing.load(Ordering::Relaxed),
                level: [
                    f32::from_bits(s.level[0].load(Ordering::Relaxed)),
                    f32::from_bits(s.level[1].load(Ordering::Relaxed)),
                ],
            };
        }
        snap
    }
}
