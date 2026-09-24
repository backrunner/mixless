//! Device ownership, engine construction and decoded-track lifecycle.

use super::*;

fn spawn_output_thread(
    shared: Arc<Shared>,
) -> Result<std::sync::mpsc::Sender<AudioRequest>, EngineError> {
    let (tx, rx) = std::sync::mpsc::channel::<AudioRequest>();
    std::thread::Builder::new()
        .name("mixless-audio".into())
        .spawn(move || {
            // Own and drop CoreAudio streams on this thread, including during reconfiguration.
            let mut current: Option<device::OutputStreams> = None;
            for (config, reply) in rx {
                let result = (|| {
                    let next = device::OutputStreams::prepare(shared.clone(), &config)?;
                    if let Some(old) = &current {
                        old.pause()?;
                    }
                    let previous_sr = shared.sample_rate.swap(next.sample_rate, Ordering::Relaxed);
                    shared.audio_failed.store(false, Ordering::Relaxed);
                    shared.cue_failed.store(false, Ordering::Relaxed);
                    if let Err(error) = next.play() {
                        shared.sample_rate.store(previous_sr, Ordering::Relaxed);
                        let restored = current.as_ref().is_some_and(|old| old.play().is_ok());
                        shared.audio_failed.store(!restored, Ordering::Relaxed);
                        return Err(error);
                    }
                    *shared.device_name.lock().expect("device name") = next.device_name.clone();
                    *shared.cue_device.lock().expect("cue device") = next.cue_name.clone();
                    current = Some(next);
                    Ok(())
                })();
                let _ = reply.send(result);
            }
        })
        .map_err(|error| device::DeviceError::Cpal(error.to_string()))?;
    Ok(tx)
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Self, EngineError> {
        Self::new_with_audio(config, device::AudioConfig::default())
    }

    pub fn new_with_audio(
        config: EngineConfig,
        audio: device::AudioConfig,
    ) -> Result<Self, EngineError> {
        let sr = config.sample_rate;
        let shared = Arc::new(Shared {
            presentation: presentation::PresentationClock::default(),
            audio_failed: AtomicBool::new(true),
            cue_failed: AtomicBool::new(false),
            cue_device: Mutex::new(None),
            sync_follower: AtomicU32::new(0),
            sync_factor: AtomicU32::new(1.0_f32.to_bits()),
            sync_align: AtomicBool::new(false),
            sync_locked: AtomicBool::new(false),
            sample_rate: AtomicU32::new(sr),
            block_frames: AtomicU32::new(config.block_frames),
            decks: [DeckSlot::new(), DeckSlot::new()],
            xfader: AtomicU32::new(500),
            xf_curve: AtomicU32::new(1),
            xf_reverse: AtomicBool::new(false),
            master: AtomicU32::new(800),
            master_gain_centi: AtomicU32::new(1200),
            master_level: std::array::from_fn(|_| AtomicU32::new(0)),
            recorder: recording::Recorder::default(),
            cue_gain: AtomicU32::new(800),
            quantize: AtomicBool::new(true),
            fx_auto_fade: AtomicBool::new(true),
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

        // On macOS std::Mutex lazily allocates its pthread storage, even for
        // try_lock. Initialize callback-visible locks before opening streams.
        for slot in &shared.decks {
            drop(slot.buffer.lock().expect("buffer initialization"));
            drop(slot.stems.lock().expect("stem initialization"));
            drop(slot.beat_grid.lock().expect("grid initialization"));
        }
        drop(
            shared
                .recorder
                .sink
                .lock()
                .expect("recorder initialization"),
        );
        drop(shared.last_block.lock().expect("meter initialization"));
        shared.automation.initialize_callback_lock();
        let actual_sr = shared.sample_rate.load(Ordering::Relaxed) as f32;
        let engine = Self {
            audio_tx: if config.offline {
                None
            } else {
                Some(spawn_output_thread(shared.clone())?)
            },
            audio_config: Mutex::new(device::AudioConfig::default()),
            audio_apply: Mutex::new(()),
            shared,
            rt: Mutex::new(AudioRt::new(actual_sr)),
            retired_buffers: Mutex::new(Vec::new()),
            retired_stems: Mutex::new(Vec::new()),
        };
        if !config.offline {
            if let Err(error) = engine.configure_audio(audio) {
                tracing::warn!("saved audio configuration unavailable: {error}");
                if let Err(error) = engine.configure_audio(device::AudioConfig::default()) {
                    tracing::warn!("audio unavailable: {error}");
                }
            }
        }
        Ok(engine)
    }

    pub fn audio_config(&self) -> device::AudioConfig {
        self.audio_config.lock().expect("audio config").clone()
    }

    pub fn audio_available(&self) -> bool {
        !self.shared.audio_failed.load(Ordering::Relaxed)
    }

    /// Called off the UI/audio threads. Failure keeps the previous stream configuration.
    pub fn configure_audio(&self, config: device::AudioConfig) -> Result<(), EngineError> {
        let tx = self
            .audio_tx
            .as_ref()
            .ok_or(EngineError::Protocol("offline engine has no audio devices"))?;
        let _apply = self.audio_apply.lock().expect("audio apply");
        if self.recording_status().active {
            return Err(EngineError::Protocol(
                "Stop recording before changing audio devices",
            ));
        }
        let (reply, result) = std::sync::mpsc::channel();
        tx.send((config.clone(), reply))
            .map_err(|_| EngineError::Protocol("audio thread stopped"))?;
        result
            .recv()
            .map_err(|_| EngineError::Protocol("audio thread stopped"))??;
        *self.audio_config.lock().expect("audio config") = config;
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
        self.load_buffer_if(deck, track_id, buf, wave, title, artist, should_commit)
    }

    /// Publish predecoded audio and its cached waveform; all preparation is host-side.
    pub fn load_buffer_if(
        &self,
        deck: DeckId,
        track_id: TrackId,
        buf: Arc<AudioBuffer>,
        wave: Arc<mixless_protocol::Waveform>,
        title: String,
        artist: String,
        should_commit: impl FnOnce() -> bool,
    ) -> Result<bool, EngineError> {
        if !should_commit() {
            return Ok(false);
        }
        self.automation_command(&Command::StopAutomix);
        self.shared.clear_sync();
        self.clear_stems(deck);
        let slot = &self.shared.decks[deck.index()];
        *slot
            .beat_grid
            .lock()
            .map_err(|_| EngineError::Protocol("beat grid lock poisoned"))? = None;
        slot.loop_on.store(false, Ordering::Relaxed);
        for cue in &slot.cues {
            cue.store(0, Ordering::Relaxed);
        }
        slot.bpm_milli.store(0, Ordering::Relaxed);
        slot.frames.store(buf.frames, Ordering::Relaxed);
        slot.src_sr.store(buf.sample_rate, Ordering::Relaxed);
        slot.set_playhead(0.0);
        slot.playing.store(false, Ordering::Relaxed);
        // A new source starts at unity trim, including decks previously faded
        // to silence by an older automation plan. Channel faders retain position.
        slot.gain_milli.store(9600, Ordering::Relaxed);
        slot.automix_gain_centi.store(0, Ordering::Relaxed);
        slot.jog_touch.store(false, Ordering::Release);
        slot.seek_pending.store(false, Ordering::Release);
        slot.temporary_cue.store(0, Ordering::Relaxed);
        slot.preview_cue.store(0, Ordering::Relaxed);
        slot.brake.store(false, Ordering::Relaxed);
        slot.roll.store(false, Ordering::Relaxed);
        for kind in &slot.cue_kinds {
            kind.store(0, Ordering::Relaxed);
        }
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
        // Publish identity after the waveform and labels; otherwise the UI can
        // cache the previous waveform under this track's ID for the whole load.
        slot.track_id.store(track_id.0 as u64, Ordering::Release);
        slot.load_revision.fetch_add(1, Ordering::AcqRel);
        Ok(true)
    }

    pub fn eject(&self, deck: DeckId) {
        self.stop_automix_for_eject();
        self.shared.clear_sync();
        let slot = &self.shared.decks[deck.index()];
        if let Ok(mut grid) = slot.beat_grid.lock() {
            *grid = None;
        }
        slot.playing.store(false, Ordering::Relaxed);
        slot.jog_touch.store(false, Ordering::Release);
        slot.seek_pending.store(false, Ordering::Release);
        slot.temporary_cue.store(0, Ordering::Relaxed);
        slot.preview_cue.store(0, Ordering::Relaxed);
        slot.brake.store(false, Ordering::Relaxed);
        slot.roll.store(false, Ordering::Relaxed);
        for kind in &slot.cue_kinds {
            kind.store(0, Ordering::Relaxed);
        }
        slot.track_id.store(0, Ordering::Relaxed);
        slot.frames.store(0, Ordering::Relaxed);
        slot.src_sr.store(0, Ordering::Relaxed);
        slot.bpm_milli.store(0, Ordering::Relaxed);
        slot.loop_on.store(false, Ordering::Relaxed);
        slot.loop_start.store(0, Ordering::Relaxed);
        slot.loop_length_frames.store(0, Ordering::Relaxed);
        slot.roll_start.store(0, Ordering::Relaxed);
        for level in &slot.level {
            level.store(0, Ordering::Relaxed);
        }
        slot.set_playhead(0.0);
        if let Ok(mut buffer) = slot.buffer.lock() {
            self.retire_buffer(buffer.take());
        }
        // attach_stems holds buffer through publication. Clear it only after
        // removing the source, so an in-flight attachment cannot win afterwards.
        self.clear_stems(deck);
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
        slot.load_revision.fetch_add(1, Ordering::AcqRel);
    }

    /// Changes even when the same track and cached PCM are loaded again.
    pub fn deck_load_revision(&self, deck: DeckId) -> u64 {
        self.shared.decks[deck.index()]
            .load_revision
            .load(Ordering::Acquire)
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

    /// Position a prepared, paused deck without overwriting a user's hot cues.
    pub fn cue_loaded_track(
        &self,
        deck: DeckId,
        id: TrackId,
        frame: u64,
    ) -> Result<(), EngineError> {
        let slot = &self.shared.decks[deck.index()];
        if slot.track_id.load(Ordering::Relaxed) != id.0 as u64
            || slot.playing.load(Ordering::Relaxed)
        {
            return Err(EngineError::Protocol(
                "automatic cue requires the prepared paused deck",
            ));
        }
        let frame = frame.min(slot.frames.load(Ordering::Relaxed).saturating_sub(1));
        slot.automix_cue.store(frame + 1, Ordering::Relaxed);
        slot.seek_to.store(frame * 65536, Ordering::Relaxed);
        slot.seek_pending.store(true, Ordering::Release);
        Ok(())
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
    pub fn deck_waveform(&self, deck: DeckId) -> Option<Arc<mixless_protocol::Waveform>> {
        self.shared.decks[deck.index()]
            .waveform
            .lock()
            .ok()
            .and_then(|w| w.clone())
    }

    /// Replace a temporary envelope after background analysis, without reloading audio.
    /// Host callers serialize this with deck loads and cancellation checks.
    pub fn set_waveform(
        &self,
        deck: DeckId,
        track: TrackId,
        wave: Arc<mixless_protocol::Waveform>,
    ) {
        let slot = &self.shared.decks[deck.index()];
        if let Ok(mut current) = slot.waveform.lock() {
            if slot.track_id.load(Ordering::Acquire) == track.0 as u64 {
                *current = Some(wave);
            }
        }
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
