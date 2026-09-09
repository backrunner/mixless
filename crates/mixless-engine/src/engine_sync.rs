//! Beat-grid transport synchronization. All allocation/validation is host-side;
//! the callback only tries grid locks and performs binary searches.
use super::*;
use mixless_protocol::TempoMap;

pub(super) struct BeatGrid {
    track_id: TrackId,
    tempo: TempoMap,
}

impl BeatGrid {
    fn beat_at(&self, seconds: f64) -> (f64, f64) {
        let beats = &self.tempo.beats;
        let i = beats
            .partition_point(|t| (*t as f64) <= seconds)
            .saturating_sub(1)
            .min(beats.len() - 2);
        let period = (beats[i + 1] - beats[i]) as f64;
        (i as f64 + (seconds - beats[i] as f64) / period, period)
    }

    fn time_at(&self, beat: f64) -> f64 {
        let i = (beat.floor().max(0.0) as usize).min(self.tempo.beats.len() - 2);
        self.tempo.beats[i] as f64
            + (beat - i as f64) * (self.tempo.beats[i + 1] - self.tempo.beats[i]) as f64
    }

    pub(super) fn bpm_at_seconds(&self, seconds: f64) -> f32 {
        let (_, period) = self.beat_at(seconds);
        (60.0 / period).clamp(20.0, 400.0) as f32
    }

    pub(super) fn fx_clock_at(&self, seconds: f64) -> (f64, f64) {
        let (beat, period) = self.beat_at(seconds);
        let origin = self
            .tempo
            .downbeats
            .first()
            .map_or(0.0, |t| self.beat_at(*t as f64).0);
        (beat - origin, period)
    }

    fn bar_span_seconds(&self, seconds: f64, bars: i16) -> Option<f64> {
        let (beat, _) = self.beat_at(seconds);
        let target = (beat + bars as f64 * self.tempo.meter_num.max(1) as f64).max(0.0);
        Some(self.time_at(target) - seconds)
    }
}

fn phase_error(master: f64, follower: f64) -> f64 {
    (master - follower + 0.5).rem_euclid(1.0) - 0.5
}

impl Engine {
    /// Seconds in the decoded source domain. Refuse malformed or stale grids
    /// instead of inventing a beat at frame zero. Called off the audio thread.
    pub fn set_beat_grid(
        &self,
        deck: DeckId,
        track_id: TrackId,
        tempo: TempoMap,
    ) -> Result<(), EngineError> {
        if tempo.beats.len() < 2
            || !tempo.beats.iter().all(|t| t.is_finite() && *t >= 0.0)
            || !tempo.beats.windows(2).all(|w| w[1] - w[0] >= 0.05)
            || !tempo.downbeats.iter().all(|t| t.is_finite() && *t >= 0.0)
            || !tempo.downbeats.windows(2).all(|w| w[1] > w[0])
            || !tempo.segments.iter().all(|s| {
                s.start_beat.is_finite()
                    && s.end_beat.is_finite()
                    && s.end_beat > s.start_beat
                    && s.start_beat >= 0.0
                    && s.bpm.is_finite()
                    && (20.0..=400.0).contains(&s.bpm)
                    && s.confidence.is_finite()
                    && (0.0..=1.0).contains(&s.confidence)
            })
            || tempo
                .segments
                .windows(2)
                .any(|w| w[1].start_beat < w[0].end_beat)
        {
            return Err(EngineError::Protocol(
                "Beat Sync needs a valid analyzed beat grid",
            ));
        }
        let slot = &self.shared.decks[deck.index()];
        let mut grid = slot
            .beat_grid
            .lock()
            .map_err(|_| EngineError::Protocol("beat grid lock poisoned"))?;
        if slot.track_id.load(Ordering::Relaxed) != track_id.0 as u64 {
            return Err(EngineError::Protocol(
                "track changed while analyzing beat grid",
            ));
        }
        *grid = Some(BeatGrid { track_id, tempo });
        self.shared.sync_align.store(true, Ordering::Release);
        Ok(())
    }

    pub(super) fn enable_sync(&self, deck: DeckId, keylock: bool) -> Result<(), EngineError> {
        let i = deck.index();
        let o = 1 - i;
        let own = &self.shared.decks[i];
        let other = &self.shared.decks[o];
        // Fixed lock order also permits concurrent controller/UI requests.
        let grids = [
            self.shared.decks[0]
                .beat_grid
                .lock()
                .map_err(|_| EngineError::Protocol("beat grid lock poisoned"))?,
            self.shared.decks[1]
                .beat_grid
                .lock()
                .map_err(|_| EngineError::Protocol("beat grid lock poisoned"))?,
        ];
        let (Some(a), Some(b)) = (grids[i].as_ref(), grids[o].as_ref()) else {
            return Err(EngineError::Protocol(
                "Beat Sync needs analyzed beat grids on both decks",
            ));
        };
        if own.frames.load(Ordering::Relaxed) == 0 || other.frames.load(Ordering::Relaxed) == 0 {
            return Err(EngineError::EmptyDeck);
        }
        if own.reverse.load(Ordering::Relaxed)
            || other.reverse.load(Ordering::Relaxed)
            || own.jog_touch.load(Ordering::Acquire)
            || other.jog_touch.load(Ordering::Acquire)
        {
            return Err(EngineError::Protocol(
                "Release the waveform/jog and turn off reverse before syncing",
            ));
        }
        let (_, period_a) =
            a.beat_at(own.playhead_frames() / own.src_sr.load(Ordering::Relaxed) as f64);
        let (_, period_b) =
            b.beat_at(other.playhead_frames() / other.src_sr.load(Ordering::Relaxed) as f64);
        let ratio = period_a / period_b * other.rate_micro.load(Ordering::Relaxed) as f64 / 1e6;
        let factor = [1.0_f64, 0.5, 2.0]
            .into_iter()
            .min_by(|a, b| (ratio * a).ln().abs().total_cmp(&(ratio * b).ln().abs()))
            .unwrap();
        if !(0.25..=4.0).contains(&(ratio * factor)) {
            return Err(EngineError::Protocol(
                "Beat Sync tempo is outside the supported range",
            ));
        }
        // A single follower ID makes circular master/follower relationships impossible.
        self.shared.sync_follower.store(0, Ordering::Release);
        self.automation_command(&Command::StopAutomix);
        own.keylock.store(keylock, Ordering::Relaxed);
        own.rate_micro
            .store((ratio * factor * 1e6).round() as u32, Ordering::Relaxed);
        self.shared
            .sync_factor
            .store((factor as f32).to_bits(), Ordering::Relaxed);
        self.shared.sync_align.store(true, Ordering::Release);
        self.shared.sync_locked.store(false, Ordering::Relaxed);
        self.shared
            .sync_follower
            .store(i as u32 + 1, Ordering::Release);
        Ok(())
    }
}

impl Shared {
    pub(super) fn beat_jump_frames(&self, index: usize, bars: i16) -> Option<f64> {
        let slot = &self.decks[index];
        let sr = slot.src_sr.load(Ordering::Relaxed).max(1) as f64;
        let seconds = slot.playhead_frames() / sr;
        let grid = slot.beat_grid.try_lock().ok()?;
        let span = grid.as_ref()?.bar_span_seconds(seconds, bars)?;
        Some(span * sr)
    }

    pub(super) fn configure_loop(&self, index: usize, beats: f32, on: bool) {
        let slot = &self.decks[index];
        let beats = ((beats.clamp(0.0625, 64.) * 16.).round() / 16.) as f64;
        let sr = slot.src_sr.load(Ordering::Relaxed).max(1) as f64;
        let frames = slot.frames.load(Ordering::Relaxed) as f64;
        let was_on = slot.loop_on.load(Ordering::Relaxed);
        let mut start = if was_on { slot.loop_start.load(Ordering::Relaxed) as f64 / 65536. }
            else { slot.playhead_frames() };
        let mut bpm = slot.bpm_milli.load(Ordering::Relaxed) as f64 / 100.;
        if bpm < 1. { bpm = 120.; }
        let mut length = beats * 60. / bpm * sr;
        if let Ok(grid) = slot.beat_grid.lock() {
            if let Some(grid) = grid.as_ref() {
                let mut beat = grid.beat_at(start / sr).0;
                if !was_on && self.quantize.load(Ordering::Relaxed) {
                    let quantum = beats.min(1.);
                    beat = (beat / quantum).round() * quantum;
                    start = (grid.time_at(beat) * sr).max(0.);
                }
                length = (grid.time_at(beat + beats) * sr - start).max(1.);
            }
        }
        length = length.min(frames).max(0.);
        start = start.clamp(0., (frames - length).max(0.));
        slot.loop_start.store((start * 65536.).round() as u64, Ordering::Relaxed);
        slot.loop_length_frames.store(length.round() as u64, Ordering::Relaxed);
        slot.loop_sixteenths.store((beats * 16.) as u32, Ordering::Relaxed);
        slot.loop_on.store(on && length > 1., Ordering::Release);
    }

    pub(super) fn grid_snapshot(&self, index: usize) -> (f32, f32, Option<f32>, bool) {
        let slot = &self.decks[index];
        let Ok(grid) = slot.beat_grid.lock() else {
            return (0.0, 0.0, None, false);
        };
        let Some(grid) = grid.as_ref() else {
            return (0.0, 0.0, None, false);
        };
        let (beat, period) =
            grid.beat_at(slot.playhead_frames() / slot.src_sr.load(Ordering::Relaxed) as f64);
        let bar = grid.tempo.downbeats.first().map_or(0.0, |downbeat| {
            (beat - grid.beat_at(*downbeat as f64).0) / grid.tempo.meter_num.max(1) as f64
        });
        (beat as f32, bar as f32, Some((60.0 / period) as f32), true)
    }

    pub(super) fn clear_sync(&self) {
        self.sync_follower.store(0, Ordering::Release);
        self.sync_locked.store(false, Ordering::Relaxed);
    }

    /// Nominal tempo is published separately from the small phase correction.
    /// Never seek repeatedly: hard-align only on activation/start/cue, then
    /// correct drift with at most 2% temporary tempo bend.
    pub(super) fn sync_rates(&self) -> [f32; 2] {
        let mut bend = [1.0; 2];
        let follower = self.sync_follower.load(Ordering::Acquire);
        if follower == 0 {
            return bend;
        }
        let i = (follower - 1) as usize;
        let own = &self.decks[i];
        let master = &self.decks[1 - i];
        let (Ok(a), Ok(b)) = (own.beat_grid.try_lock(), master.beat_grid.try_lock()) else {
            return bend;
        };
        let (Some(a), Some(b)) = (a.as_ref(), b.as_ref()) else {
            self.clear_sync();
            return bend;
        };
        if a.track_id.0 as u64 != own.track_id.load(Ordering::Relaxed)
            || b.track_id.0 as u64 != master.track_id.load(Ordering::Relaxed)
        {
            self.clear_sync();
            return bend;
        }
        let source_sr = own.src_sr.load(Ordering::Relaxed) as f64;
        let pos = |slot: &DeckSlot| {
            if slot.seek_pending.load(Ordering::Acquire) {
                slot.seek_to.load(Ordering::Relaxed) as f64 / 65536.0
            } else {
                slot.playhead_frames()
            }
        };
        let (beat, period) = a.beat_at(pos(own) / source_sr);
        let (master_beat, master_period) =
            b.beat_at(pos(master) / master.src_sr.load(Ordering::Relaxed) as f64);
        let factor = f32::from_bits(self.sync_factor.load(Ordering::Relaxed)) as f64;
        let rate =
            period / master_period * factor * master.rate_micro.load(Ordering::Relaxed) as f64
                / 1e6;
        if !(0.25..=4.0).contains(&rate) {
            self.clear_sync();
            return bend;
        }
        own.rate_micro
            .store((rate * 1e6).round() as u32, Ordering::Relaxed);
        if !own.playing.load(Ordering::Relaxed)
            || !master.playing.load(Ordering::Relaxed)
            || own.jog_touch.load(Ordering::Acquire)
            || master.jog_touch.load(Ordering::Acquire)
        {
            self.sync_align.store(true, Ordering::Release);
            self.sync_locked.store(false, Ordering::Relaxed);
            return bend;
        }
        let error = phase_error(master_beat * factor, beat);
        if self.sync_align.swap(false, Ordering::AcqRel) {
            let target = a.time_at(beat + error) * source_sr;
            if target >= 0.0 && target < own.frames.load(Ordering::Relaxed) as f64 {
                own.seek_to
                    .store((target * 65536.0) as u64, Ordering::Relaxed);
                own.seek_pending.store(true, Ordering::Release);
            } else {
                // At the file edge wait until an aligned source position exists.
                self.sync_align.store(true, Ordering::Release);
                self.sync_locked.store(false, Ordering::Relaxed);
                return bend;
            }
        } else {
            bend[i] = (1.0 + (error * period / rate * 2.0).clamp(-0.02, 0.02)) as f32;
        }
        self.sync_locked
            .store(error.abs() < 0.02, Ordering::Relaxed);
        bend
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(periods: [f32; 2], offsets: [f32; 2], rates: [u32; 2]) -> Engine {
        let engine = Engine::new(EngineConfig {
            offline: true,
            ..Default::default()
        })
        .unwrap();
        for i in 0..2 {
            let deck = if i == 0 { DeckId::A } else { DeckId::B };
            let slot = &engine.shared.decks[i];
            let frames = rates[i] as usize * 90;
            let mut samples = vec![0.0; frames * 2];
            let beats: Vec<_> = (0..)
                .map(|n| offsets[i] + n as f32 * periods[i])
                .take_while(|t| *t < 90.0)
                .collect();
            // Actual transients at the measured grid, not just a synthetic
            // playhead counter. Both sources may use different sample rates.
            for beat in &beats {
                let first = (*beat * rates[i] as f32) as usize;
                for n in 0..(rates[i] / 100) as usize {
                    let value = (1.0 - n as f32 / (rates[i] / 100) as f32) * 0.3;
                    if first + n < frames {
                        samples[(first + n) * 2] = value;
                        samples[(first + n) * 2 + 1] = value;
                    }
                }
            }
            *slot.buffer.lock().unwrap() = Some(Arc::new(AudioBuffer {
                samples,
                frames: frames as u64,
                sample_rate: rates[i],
            }));
            slot.frames.store(frames as u64, Ordering::Relaxed);
            slot.src_sr.store(rates[i], Ordering::Relaxed);
            slot.track_id.store(i as u64 + 1, Ordering::Relaxed);
            slot.set_playhead(rates[i] as f64 * (2.0 + i as f64 * 0.17));
            engine.set_bpm(deck, 60.0 / periods[i]);
            engine
                .set_beat_grid(
                    deck,
                    TrackId(i as i64 + 1),
                    TempoMap {
                        global_bpm: 60.0 / periods[i],
                        meter_num: 4,
                        meter_den: 4,
                        downbeats: beats.iter().step_by(4).copied().collect(),
                        beats,
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        engine
    }

    #[test]
    fn local_grid_controls_bar_jump_and_loop_span() {
        let engine = setup([0.5, 0.5], [0.0, 0.0], [48_000; 2]);
        let slot = &engine.shared.decks[0];
        let mut beats = Vec::new();
        let mut t = 0.0;
        for i in 0..32 {
            beats.push(t);
            t += if i < 16 { 0.5 } else { 60.0 / 180.0 };
        }
        beats.push(t);
        let tempo = TempoMap {
            global_bpm: 120.0,
            meter_num: 4,
            meter_den: 4,
            beats,
            downbeats: vec![0.0, 2.0, 4.0, 6.0],
            segments: vec![
                mixless_protocol::TempoSegment {
                    start_beat: 0.0,
                    end_beat: 16.0,
                    bpm: 120.0,
                    confidence: 1.0,
                },
                mixless_protocol::TempoSegment {
                    start_beat: 16.0,
                    end_beat: 32.0,
                    bpm: 180.0,
                    confidence: 1.0,
                },
            ],
        };
        *slot.beat_grid.lock().unwrap() = Some(BeatGrid {
            track_id: TrackId(1),
            tempo,
        });
        slot.set_playhead(8.0 * 48_000.0 / 1.0);
        let jump = engine.shared.beat_jump_frames(0, 1).unwrap();
        assert!((jump / 48_000.0 - 4.0 / 3.0).abs() < 1e-5);
        slot.frames.store(48_000 * 60, Ordering::Relaxed);
        slot.set_playhead(16.0 * 48_000.0);
        engine.dispatch(Command::SetLoopBeats { deck: DeckId::A, beats: 4., on: true }).unwrap();
        let loop_len = slot.loop_length_frames.load(Ordering::Relaxed) as f64;
        assert!((loop_len / 48_000.0 - 4.0 / 3.0).abs() < 1e-5);
    }

    fn start(engine: &Engine) {
        for deck in [DeckId::A, DeckId::B] {
            engine.dispatch(Command::PlayPause { deck }).unwrap();
        }
        engine
            .dispatch(Command::Sync {
                deck: DeckId::B,
                keylock: true,
            })
            .unwrap();
    }

    fn error(engine: &Engine) -> f64 {
        let s = engine.snapshot();
        let factor = f32::from_bits(engine.shared.sync_factor.load(Ordering::Relaxed));
        phase_error((s.decks[0].beat * factor) as f64, s.decks[1].beat as f64).abs()
    }

    #[test]
    fn source_rate_and_offset_alignment_stays_locked_and_follows_master_tempo() {
        let engine = setup([0.5, 0.6], [0.11, 0.27], [44_100, 48_000]);
        start(&engine);
        for n in 0..1200 {
            if n == 600 {
                engine
                    .dispatch(Command::SetRate {
                        deck: DeckId::A,
                        rate: 1.08,
                    })
                    .unwrap();
            }
            let output = engine.render_offline(1024);
            assert!(output.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            if n > 80 && !(600..680).contains(&n) {
                assert!(
                    error(&engine) < 0.004,
                    "block {n}: phase {}",
                    error(&engine)
                );
            }
        }
        let s = engine.snapshot();
        assert!(s.decks[0].sync_master && s.decks[1].sync_locked);
        assert!((s.decks[1].rate - 1.296).abs() < 0.001);
        assert!((s.decks[0].sounding_bpm - s.decks[1].sounding_bpm).abs() < 0.01);
    }

    #[test]
    fn rendered_transients_align_through_keylocked_stretch() {
        let isolated = |crossfader| {
            let engine = setup([0.5, 0.6], [0.11, 0.27], [44_100, 48_000]);
            engine
                .dispatch(Command::SetCrossfader { value: crossfader })
                .unwrap();
            start(&engine);
            engine.render_offline(48_000);
            engine.render_offline(48_000 * 3)
        };
        let (master, follower) = (isolated(-1.0), isolated(1.0));
        for (a, b) in master
            .chunks_exact(48_000)
            .zip(follower.chunks_exact(48_000))
        {
            let peak = |wave: &[f32]| {
                wave.iter()
                    .enumerate()
                    .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                    .unwrap()
                    .0
                    / 2
            };
            let delta = peak(a).abs_diff(peak(b));
            assert!(
                delta < 480,
                "audible transients differ by {} ms",
                delta as f64 / 48.0
            );
        }
    }

    #[test]
    fn local_tempo_changes_follow_the_grid_and_master_can_switch() {
        let engine = setup([0.5, 0.6], [0.1, 0.3], [48_000, 44_100]);
        {
            let mut grid = engine.shared.decks[0].beat_grid.lock().unwrap();
            // Gradual tempo variation, then a faster constant region.
            let beats = &mut grid.as_mut().unwrap().tempo.beats;
            for i in 1..beats.len() {
                beats[i] = beats[i - 1] + (0.5 - i as f32 * 0.001).max(0.45);
            }
        }
        start(&engine);
        engine.render_offline(48_000 * 15);
        assert!(error(&engine) < 0.01);
        engine
            .dispatch(Command::Sync {
                deck: DeckId::A,
                keylock: true,
            })
            .unwrap();
        engine.render_offline(48_000 * 2);
        let s = engine.snapshot();
        assert!(s.decks[1].sync_master && s.decks[0].sync_locked);
        assert!(!s.decks[1].synced);
        assert!(
            phase_error(s.decks[1].beat as f64, s.decks[0].beat as f64).abs() < 0.02,
            "phase after master switch: {:?}",
            s.decks
                .iter()
                .map(|d| (d.beat, d.rate, d.sync_locked))
                .collect::<Vec<_>>()
        );
        let rate = s.decks[0].rate;
        engine
            .dispatch(Command::DisableSync { deck: DeckId::A })
            .unwrap();
        engine.render_offline(256);
        assert!(!engine.snapshot().decks[0].synced);
        assert_eq!(engine.snapshot().decks[0].rate, rate);
    }

    #[test]
    fn paused_follower_arms_without_seeking_then_aligns_on_start_and_cue() {
        let engine = setup([0.5, 0.6], [0.1, 0.3], [48_000; 2]);
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        engine
            .dispatch(Command::Sync {
                deck: DeckId::B,
                keylock: true,
            })
            .unwrap();
        let frame = engine.snapshot().decks[1].frame;
        engine.render_offline(48_000);
        let s = engine.snapshot();
        assert_eq!(s.decks[1].frame, frame);
        assert!(s.decks[1].synced && !s.decks[1].sync_locked);
        engine
            .dispatch(Command::PlayPause { deck: DeckId::B })
            .unwrap();
        engine.render_offline(48_000);
        assert!(error(&engine) < 0.005);
        engine.set_cue_frame(DeckId::B, 0, 48_000 * 12 + 7000);
        engine
            .dispatch(Command::JumpCue {
                deck: DeckId::B,
                index: 0,
            })
            .unwrap();
        engine.render_offline(48_000);
        assert!(error(&engine) < 0.005);
        assert!(engine.snapshot().decks[1].playing);
    }

    #[test]
    fn half_time_match_and_manual_takeover_do_not_jump_back() {
        let engine = setup([0.5, 1.0], [0.0, 0.2], [48_000; 2]);
        start(&engine);
        engine.render_offline(48_000);
        assert!((engine.snapshot().decks[1].rate - 1.0).abs() < 0.001);
        assert!(error(&engine) < 0.005);
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::B,
                touching: true,
            })
            .unwrap();
        let before = engine.snapshot().decks[1].frame;
        engine
            .dispatch(Command::Jog {
                deck: DeckId::B,
                delta_frames: 12_000.0,
            })
            .unwrap();
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::B,
                touching: false,
            })
            .unwrap();
        engine.render_offline(128);
        let s = engine.snapshot();
        assert!(!s.decks[1].synced && !s.decks[0].sync_master);
        assert!((s.decks[1].frame as i64 - before as i64 - 12_128).abs() < 3);
        assert!(s.decks[1].playing);
    }

    #[test]
    fn release_between_callbacks_keeps_final_target_and_paused_state() {
        let engine = setup([0.5; 2], [0.0; 2], [44_100, 48_000]);
        let before = engine.snapshot().decks[0].frame;
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: true,
            })
            .unwrap();
        engine
            .dispatch(Command::Jog {
                deck: DeckId::A,
                delta_frames: -11_025.0,
            })
            .unwrap();
        engine
            .dispatch(Command::SetJogTouch {
                deck: DeckId::A,
                touching: false,
            })
            .unwrap();
        engine.render_offline(256);
        let s = engine.snapshot();
        assert_eq!(s.decks[0].frame, before - 11_025);
        assert!(!s.decks[0].playing);
    }

    #[test]
    fn invalid_missing_and_stale_grids_cannot_claim_sync() {
        let engine = super::super::tests::test_engine(48_000);
        assert!(engine
            .dispatch(Command::Sync {
                deck: DeckId::A,
                keylock: true
            })
            .is_err());
        for beats in [
            vec![],
            vec![0.0],
            vec![0.0, f32::NAN],
            vec![0.5, 0.4],
            vec![0.0, 0.0],
        ] {
            assert!(engine
                .set_beat_grid(
                    DeckId::A,
                    TrackId(0),
                    TempoMap {
                        beats,
                        ..Default::default()
                    }
                )
                .is_err());
        }
        assert!(engine
            .set_beat_grid(
                DeckId::A,
                TrackId(99),
                TempoMap {
                    beats: vec![0.0, 0.5],
                    ..Default::default()
                }
            )
            .is_err());
        assert!(!engine.snapshot().decks[0].synced);
    }

    #[test]
    fn grid_contention_does_not_block_audio_and_eject_clears_relationship() {
        let engine = setup([0.5; 2], [0.1, 0.3], [48_000; 2]);
        start(&engine);
        engine.render_offline(4096);
        let before = engine.snapshot().decks[1].frame;
        {
            let _guard = engine.shared.decks[0].beat_grid.lock().unwrap();
            engine.render_offline(256);
        }
        assert!(engine.snapshot().decks[1].frame > before);
        engine.eject(DeckId::A);
        assert!(!engine.snapshot().decks[1].synced);
        assert!(!engine.snapshot().decks[0].grid_ready);
    }
}
