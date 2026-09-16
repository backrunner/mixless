//! Callback channel conversion, block preparation, sends and master mixing.

use super::*;

impl Shared {
    pub(crate) fn render_interleaved(
        shared: &Shared,
        rt: &mut AudioRt,
        data: &mut [f32],
        channels: usize,
        playback: std::time::Instant,
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
        let period = std::time::Duration::from_secs_f64(
            frames as f64 / shared.sample_rate.load(Ordering::Relaxed).max(1) as f64,
        );
        shared.publish_presentation(rt, playback + period, period);
    }

    pub(super) fn process_audio_block(&self, rt: &mut AudioRt, out: &mut [f32], channels: usize) {
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

        let sync_bend = self.sync_rates();
        for index in 0..2 {
            let slot = &self.decks[index];
            let deck = &mut rt.decks[index];
            let buffer_id = bufs[index]
                .as_ref()
                .map_or(0, |buffer| Arc::as_ptr(buffer) as usize);
            if buffer_id != deck.buffer_id {
                deck.buffer_id = buffer_id;
                deck.fx_clock = None;
                deck.source = bufs[index].clone();
                deck.stems = None;
                deck.stem_gain = std::array::from_fn(|_| SmoothValue::new(1., sr, 0.15));
                deck.stem_values = [1.; 3];
                deck.position = slot.playhead_frames();
                deck.old_ph = deck.position;
                deck.seek = SeekXf::new();
                deck.brake_elapsed = None;
                deck.paused_seek = false;
                deck.touching = false;
                deck.isolator_l = Isolator::new(sr);
                deck.isolator_r = Isolator::new(sr);
                deck.filter_l = ChannelFilter::new(sr);
                deck.filter_r = ChannelFilter::new(sr);
                deck.amplitude = SmoothValue::new(0.0, sr, 0.001);
                let trim = db_to_lin(slot.gain_milli.load(Ordering::Relaxed) as f32 / 100. - 96.);
                deck.gain = SmoothValue::new(
                    trim * slot.fader.load(Ordering::Relaxed) as f32 / 1000.,
                    sr,
                    0.003,
                );
                let balance = slot.balance_milli.load(Ordering::Relaxed) as f32 / 500. - 1.;
                deck.balance_l = SmoothValue::new(1. - balance.max(0.), sr, 0.003);
                deck.balance_r = SmoothValue::new(1. + balance.min(0.), sr, 0.003);
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
            if let Ok(stems) = slot.stems.try_lock() {
                deck.stems = stems
                    .as_ref()
                    .filter(|s| {
                        bufs[index]
                            .as_ref()
                            .is_some_and(|b| Arc::ptr_eq(b, &s.source))
                    })
                    .cloned();
            }
            for i in 0..3 {
                deck.stem_gain[i].set(if deck.stems.is_some() {
                    slot.stem_gain[i].load(Ordering::Relaxed) as f32 / 1000.
                } else {
                    1.
                });
            }
            deck.playing = slot.playing.load(Ordering::Relaxed);
            if deck.playing {
                deck.paused_seek = false;
            }
            if deck.playing && !was_playing {
                deck.music.invalidate();
            }
            let source_rate = bufs[index]
                .as_ref()
                .map_or(sr, |buffer| buffer.sample_rate as f32);
            let rate =
                slot.rate_micro.load(Ordering::Relaxed) as f32 / 1_000_000.0 * sync_bend[index];
            let pitch = slot.pitch_centi.load(Ordering::Relaxed) as f32 / 100.0 - 24.0;
            let direction = if slot.reverse.load(Ordering::Relaxed) {
                -1.0
            } else {
                1.0
            };
            deck.step_target = source_rate / sr * rate * direction;
            let braking = slot.brake.load(Ordering::Relaxed) && deck.playing;
            deck.brake_elapsed = if braking {
                Some(deck.brake_elapsed.unwrap_or(0))
            } else if !deck.playing {
                // Freeze the last slowed speed through the de-click tail.
                // Returning to native speed here would chirp on release.
                deck.brake_elapsed
            } else {
                None
            };
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
            let fallback_bpm = if bpm_raw > 0.0 { bpm_raw } else { 120.0 };
            // A loaded analysis grid is authoritative for tempo-synced loops
            // and inserts. Read the period at the current source position so
            // a section change updates BPM without a host/UI round trip.
            let bpm = slot
                .beat_grid
                .try_lock()
                .ok()
                .and_then(|grid| {
                    grid.as_ref()
                        .map(|g| g.bpm_at_seconds(deck.position / source_rate as f64))
                })
                .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
                .unwrap_or(fallback_bpm);
            let previous_loop = deck.loop_range;
            deck.loop_range = if slot.loop_on.load(Ordering::Relaxed) {
                let start = slot.loop_start.load(Ordering::Relaxed) as f64 / 65536.0;
                let length = slot.loop_length_frames.load(Ordering::Relaxed) as f64;
                let end = (start + length).min(slot.frames.load(Ordering::Relaxed) as f64);
                (end - start > 1.0).then_some((start, end))
            } else {
                None
            };
            let keylock = slot.keylock.load(Ordering::Relaxed);
            let music_mode = deck.playing
                && !braking
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
            let trim = db_to_lin(slot.gain_milli.load(Ordering::Relaxed) as f32 / 100.0 - 96.0);
            deck.cue_trim.set(trim);
            deck.gain
                .set(trim * slot.fader.load(Ordering::Relaxed) as f32 / 1000.0);
            // Linear balance: center passes both channels at unity, each
            // extreme silences the opposite side.
            let balance = slot.balance_milli.load(Ordering::Relaxed) as f32 / 500.0 - 1.0;
            deck.balance_l.set(1.0 - balance.max(0.0));
            deck.balance_r.set(1.0 + balance.min(0.0));
            let amount = slot.filter_milli.load(Ordering::Relaxed) as f32 / 500.0 - 1.0;
            let resonance = if slot.resonance_enabled.load(Ordering::Relaxed) {
                slot.resonance_milli.load(Ordering::Relaxed) as f32 / 1000.0
            } else {
                0.
            };
            deck.isolator_l.set_resonance(resonance);
            deck.isolator_r.set_resonance(resonance);
            deck.filter_l.set_amount(sr, amount);
            deck.filter_r.set_amount(sr, amount);
            deck.filter_l.set_resonance(resonance);
            deck.filter_r.set_resonance(resonance);
            for (effect, params) in deck.inserts.iter_mut().zip(&slot.inserts) {
                let mut p = params.read(bpm * rate);
                p.auto_fade = self.fx_auto_fade.load(Ordering::Relaxed);
                effect.configure(p);
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
                deck.paused_seek = !deck.playing;
                deck.seek.start(xf_len);
            }
            if let Ok(grid) = slot.beat_grid.try_lock() {
                deck.fx_clock = grid.as_ref().map(|grid| {
                    let (beat, period) = grid.fx_clock_at(deck.position / source_rate as f64);
                    (deck.position, beat, 1.0 / (period * source_rate as f64))
                });
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
        let fallback_tempo = clock.bpm_milli.load(Ordering::Relaxed) as f32 / 100.0;
        let source_rate = clock.src_sr.load(Ordering::Relaxed).max(1) as f64;
        let tempo = clock
            .beat_grid
            .try_lock()
            .ok()
            .and_then(|grid| {
                grid.as_ref()
                    .map(|g| g.bpm_at_seconds(clock.playhead_frames() / source_rate))
            })
            .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
            .unwrap_or(if fallback_tempo > 0.0 {
                fallback_tempo
            } else {
                120.0
            })
            * clock.rate_micro.load(Ordering::Relaxed) as f32
            / 1_000_000.0;
        let tempo = rt
            .automation
            .tempo
            .filter(|_| self.automation.enabled.load(Ordering::Acquire))
            .unwrap_or(tempo);
        for index in 0..2 {
            let mut p = self.sends[index].read(tempo);
            p.auto_fade = self.fx_auto_fade.load(Ordering::Relaxed);
            rt.sends[index].configure(p);
        }

        let mut last_ok = true;
        let mut peak = [[0f32; 2]; 2];
        let mut master_peak = [0f32; 2];
        let mut recording = self.recorder.sink.try_lock().ok();
        let pfl = std::array::from_fn::<_, 2, _>(|i| self.decks[i].pfl.load(Ordering::Relaxed));
        let preview = std::array::from_fn::<_, 2, _>(|i| {
            self.decks[i].preview_cue.load(Ordering::Acquire) > 0
        });
        let cue_gain = self.cue_gain.load(Ordering::Relaxed) as f32 / 1000.0;
        for i in 0..frames {
            let (mut al, mut ar) = self.render_deck(0, &mut rt.decks[0], sr, bufs[0].as_deref());
            let (mut bl, mut br) = self.render_deck(1, &mut rt.decks[1], sr, bufs[1].as_deref());
            if let Some(cue) = &mut rt.cue_output {
                let mut sample = [0.0; 2];
                for (index, enabled) in pfl.iter().enumerate() {
                    if *enabled || preview[index] {
                        sample[0] += rt.decks[index].cue_sample[0];
                        sample[1] += rt.decks[index].cue_sample[1];
                    }
                }
                let (left, right) = rt
                    .cue_limiter
                    .process(sample[0] * cue_gain, sample[1] * cue_gain);
                cue.push([left, right]);
            }
            if preview[0] {
                (al, ar) = (0., 0.);
            }
            if preview[1] {
                (bl, br) = (0., 0.);
            }
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
            master_peak[0] = master_peak[0].max(l.abs());
            master_peak[1] = master_peak[1].max(r.abs());
            if let Some(sink) = recording.as_mut().and_then(|slot| slot.as_mut()) {
                sink.push([l, r], sr as u32);
            }
        }
        if !last_ok {
            self.xrun.fetch_add(1, Ordering::Relaxed);
        }
        // Peak-hold with per-block decay so the UI meters fall smoothly.
        let decay = (-(frames as f32) / (sr * 0.18)).exp();
        for ch in 0..2 {
            let previous = f32::from_bits(self.master_level[ch].load(Ordering::Relaxed));
            self.master_level[ch].store(
                master_peak[ch].max(previous * decay).to_bits(),
                Ordering::Relaxed,
            );
        }
        for d in 0..2 {
            if bufs[d].is_some() {
                self.decks[d].set_playhead(rt.decks[d].position);
            }
            for ch in 0..2 {
                let cur = f32::from_bits(self.decks[d].level[ch].load(Ordering::Relaxed));
                let next = peak[d][ch].max(cur * decay);
                self.decks[d].level[ch].store(next.to_bits(), Ordering::Relaxed);
            }
        }
        if let Ok(mut lb) = self.last_block.try_lock() {
            if frames > 0 {
                *lb = [out[0], out.get(1).copied().unwrap_or(0.0)];
            }
        }
    }
}
