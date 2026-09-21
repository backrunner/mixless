//! Host-side command validation and atomic control updates.

use super::*;

impl Engine {
    pub fn dispatch(&self, cmd: Command) -> Result<(), EngineError> {
        if let Command::SetCueDevice { name } = &cmd {
            let mut config = self.audio_config();
            config.cue_device = name.clone();
            return self.configure_audio(config);
        }
        let finite = match &cmd {
            Command::Jog { delta_frames, .. } => delta_frames.is_finite(),
            Command::SetChannelFilter { amount, .. } => amount.is_finite(),
            Command::SetLoopBeats { beats, .. } => {
                beats.is_finite() && (0.0625..=64.).contains(beats)
            }
            Command::SetFilterResonance { resonance, .. } => resonance.is_finite(),
            Command::SetFilter { cutoff_hz, .. } => cutoff_hz.is_finite(),
            Command::SetRate { rate, .. } => rate.is_finite(),
            Command::SetPitchSemitones { semitones, .. } => semitones.is_finite(),
            Command::SetEq { db, .. }
            | Command::SetChannelGain { db, .. }
            | Command::SetDeckLimiterGain { db, .. }
            | Command::SetMasterGain { db } => db.is_finite(),
            Command::SetCrossfader { value }
            | Command::SetMaster { value }
            | Command::SetChannelFader { value, .. }
            | Command::SetBalance { value, .. }
            | Command::SetStemGain { value, .. }
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
                        params.depth,
                        params.drive,
                        params.decay_seconds,
                        params.size,
                        params.damping,
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
        if let Command::Sync { deck, keylock } = cmd {
            return self.enable_sync(deck, keylock);
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
            | Command::StartAutomix { .. } => return Ok(()),
            _ => {}
        }
        self.automation_command(&cmd);
        // Mixer params are atomics — apply on the caller thread so a full
        // command ring cannot drop a play/fader/cue under jog spam.
        self.shared.apply_cmd(&cmd);
        Ok(())
    }

    /// Host-driven live gestures on a playing deck between transitions.
    /// Applies the control atomics directly without `automation_command`, so
    /// a committed plan never reads them as user takeover of its lanes.
    pub fn perform(&self, cmd: Command) -> Result<(), EngineError> {
        let finite = match &cmd {
            Command::SetChannelFilter { amount, .. } => amount.is_finite(),
            Command::SetStemGain { value, .. } => value.is_finite(),
            _ => {
                return Err(EngineError::Protocol(
                    "perform accepts channel filter and stem gain only",
                ));
            }
        };
        if !finite {
            return Err(EngineError::Protocol("non-finite audio parameter"));
        }
        self.shared.apply_cmd(&cmd);
        Ok(())
    }
}

impl Shared {
    fn apply_cmd(&self, cmd: &Command) {
        match *cmd {
            Command::SetJogTouch { touching: true, .. }
            | Command::SetReverse { .. }
            | Command::SetBrake { on: true, .. } => self.clear_sync(),
            Command::Jog { deck, .. } | Command::SetRate { deck, .. } => {
                if self.sync_follower.load(Ordering::Acquire) == deck.index() as u32 + 1 {
                    self.clear_sync();
                }
            }
            Command::PlayPause { .. }
            | Command::JumpCue { .. }
            | Command::TriggerTemporaryCue { .. }
            | Command::BeatJump { .. } => {
                self.sync_align.store(true, Ordering::Release);
            }
            _ => {}
        }
        match *cmd {
            Command::PlayPause { deck } => {
                let s = &self.decks[deck.index()];
                s.preview_cue.store(0, Ordering::Release);
                if s.frames.load(Ordering::Relaxed) == 0 {
                    return;
                }
                s.brake.store(false, Ordering::Relaxed);
                let next = !s.playing.load(Ordering::Relaxed);
                if next
                    && s.playhead_frames()
                        >= s.frames.load(Ordering::Relaxed).saturating_sub(1) as f64
                {
                    s.seek_to.store(0, Ordering::Relaxed);
                    s.seek_pending.store(true, Ordering::Release);
                }
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
                if !touching
                    && slot.jog_touch.load(Ordering::Acquire)
                    && !(slot.slip.load(Ordering::Relaxed) && slot.playing.load(Ordering::Relaxed))
                {
                    // Commit the last pointer target even if down/move/up all
                    // arrived between two callbacks or the scratch head lagged.
                    slot.seek_to
                        .store(slot.jog_target.load(Ordering::Acquire), Ordering::Relaxed);
                    slot.seek_pending.store(true, Ordering::Release);
                }
                slot.jog_touch.store(touching, Ordering::Release);
            }
            Command::DisableSync { deck } => {
                if self.sync_follower.load(Ordering::Acquire) == deck.index() as u32 + 1 {
                    self.clear_sync();
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
            Command::SetDeckLimiterGain { deck, db } => {
                let v = ((db.clamp(-12.0, 12.0) + 12.0) * 100.0).round() as u32;
                self.decks[deck.index()]
                    .limiter_gain_centi
                    .store(v, Ordering::Relaxed);
            }
            Command::SetBalance { deck, value } => {
                let v = ((value.clamp(-1.0, 1.0) + 1.0) * 500.0).round() as u32;
                self.decks[deck.index()]
                    .balance_milli
                    .store(v, Ordering::Relaxed);
            }
            Command::SetStemGain { deck, stem, value } => {
                self.decks[deck.index()].stem_gain[stem.index()].store(
                    (value.clamp(0., 1.) * 1000.).round() as u32,
                    Ordering::Relaxed,
                );
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
            Command::SetFilterResonanceEnabled { deck, on } => {
                self.decks[deck.index()]
                    .resonance_enabled
                    .store(on, Ordering::Relaxed);
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
            Command::SetMasterGain { db } => {
                self.master_gain_centi.store(
                    ((db.clamp(-12.0, 12.0) + 12.0) * 100.0).round() as u32,
                    Ordering::Relaxed,
                );
            }
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
            Command::SetRoll { deck, division, on } => {
                let s = &self.decks[deck.index()];
                if on && !s.roll.load(Ordering::Relaxed) {
                    s.roll_start
                        .store(s.playhead.load(Ordering::Relaxed), Ordering::Relaxed);
                }
                s.roll_division
                    .store(division.clamp(1, 128) as u32, Ordering::Relaxed);
                s.roll.store(on, Ordering::Relaxed);
            }
            Command::SetBrake { deck, on } => {
                let slot = &self.decks[deck.index()];
                if on {
                    // An ended/empty deck cannot be armed by a late hold.
                    if slot.playing.load(Ordering::Relaxed)
                        && slot.frames.load(Ordering::Relaxed) > 0
                    {
                        slot.brake.store(true, Ordering::Relaxed);
                    }
                } else if slot.brake.load(Ordering::Relaxed) {
                    slot.playing.store(false, Ordering::Relaxed);
                    slot.brake.store(false, Ordering::Relaxed);
                }
            }
            Command::SetFxAutoFade { on } => self.fx_auto_fade.store(on, Ordering::Relaxed),
            Command::SetQuantize { on } => self.quantize.store(on, Ordering::Relaxed),
            Command::JumpCue { deck, index } => {
                if (index as usize) < 8 {
                    let s = &self.decks[deck.index()];
                    let packed = s.cues[index as usize].load(Ordering::Relaxed);
                    if packed > 0 {
                        s.seek_cue(packed - 1);
                    }
                }
            }
            Command::TriggerTemporaryCue { deck } => {
                let s = &self.decks[deck.index()];
                if s.frames.load(Ordering::Relaxed) == 0 {
                    return;
                }
                let packed = s.temporary_cue.load(Ordering::Relaxed);
                if packed == 0 {
                    s.temporary_cue
                        .store(s.playhead_frames() as u64 + 1, Ordering::Relaxed);
                } else {
                    s.seek_cue(packed - 1);
                }
            }
            Command::ClearTemporaryCue { deck } => {
                self.decks[deck.index()]
                    .temporary_cue
                    .store(0, Ordering::Relaxed);
            }
            Command::BeginCuePreview { deck, frame } => {
                let s = &self.decks[deck.index()];
                if s.frames.load(Ordering::Relaxed) == 0 {
                    return;
                }
                let frame = frame.min(s.frames.load(Ordering::Relaxed).saturating_sub(1));
                self.clear_sync();
                s.preview_cue.store(frame + 1, Ordering::Release);
                s.brake.store(false, Ordering::Relaxed);
                s.loop_on.store(false, Ordering::Relaxed);
                s.seek_cue(frame);
                s.playing.store(true, Ordering::Release);
            }
            Command::EndCuePreview { deck } => {
                let s = &self.decks[deck.index()];
                if let Some(frame) = s.preview_cue.load(Ordering::Acquire).checked_sub(1) {
                    s.playing.store(false, Ordering::Release);
                    s.seek_cue(frame);
                    // Keep monitor-only routing through the release envelope;
                    // a subsequent PlayPause explicitly restores normal output.
                }
            }
            Command::SetCueKind {
                track_id,
                index,
                kind,
            } => {
                if (index as usize) < 8 {
                    for slot in &self.decks {
                        if slot.track_id.load(Ordering::Acquire) == track_id.0 as u64 {
                            slot.cue_kinds[index as usize].store(
                                match kind {
                                    CueKind::Hot => 0,
                                    CueKind::In => 1,
                                    CueKind::Out => 2,
                                },
                                Ordering::Relaxed,
                            );
                        }
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
                    self.decks[deck.index()].cue_kinds[index as usize].store(0, Ordering::Relaxed);
                }
            }
            Command::BeatJump { deck, bars } => {
                let s = &self.decks[deck.index()];
                let sr = s.src_sr.load(Ordering::Relaxed).max(1) as f64;
                let frames = self
                    .beat_jump_frames(deck.index(), bars)
                    .unwrap_or_else(|| {
                        let bpm = (s.bpm_milli.load(Ordering::Relaxed) as f64 / 100.0).max(1.0);
                        let bpm = if bpm <= 1.0 { 120.0 } else { bpm };
                        bars as f64 * 4.0 * (60.0 / bpm) * sr
                    });
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
                self.configure_loop(deck.index(), bars.max(1) as f32 * 4., on);
            }
            Command::SetLoopBeats { deck, beats, on } => {
                self.configure_loop(deck.index(), beats, on);
            }
            Command::LoopHalve { deck } | Command::LoopDouble { deck } => {
                let slot = &self.decks[deck.index()];
                let beats = slot.loop_sixteenths.load(Ordering::Relaxed) as f32 / 16.;
                let factor = if matches!(cmd, Command::LoopHalve { .. }) {
                    0.5
                } else {
                    2.
                };
                self.configure_loop(
                    deck.index(),
                    (beats * factor).clamp(0.0625, 64.),
                    slot.loop_on.load(Ordering::Relaxed),
                );
            }
            _ => {}
        }
    }
}
