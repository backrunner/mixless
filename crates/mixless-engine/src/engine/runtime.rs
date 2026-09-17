//! Preallocated deck/audio state and per-sample deck rendering.

use super::*;

impl DeckRt {
    pub(super) fn new(sr: f32) -> Self {
        Self {
            presentation_step: 0.,
            cue_sample: [0.0; 2],
            trim: SmoothValue::new(1.0, sr, 0.003),
            limiter_gain: SmoothValue::new(1.0, sr, 0.003),
            automix_gain: SmoothValue::new(1.0, sr, 0.15),
            limiter: MasterLimiter::new(sr),
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
            stems: None,
            stem_gain: std::array::from_fn(|_| SmoothValue::new(1., sr, 0.15)),
            stem_values: [1.; 3],
            touching: false,
            scratch_speed: 0.0,
            slip_position: 0.0,
            amplitude: SmoothValue::new(0.0, sr, 0.001),
            fader: SmoothValue::new(1.0, sr, 0.003),
            balance_l: SmoothValue::new(1.0, sr, 0.003),
            balance_r: SmoothValue::new(1.0, sr, 0.003),
            eq: std::array::from_fn(|_| SmoothValue::new(1.0, sr, 0.005)),
            step: SmoothValue::new(1.0, sr, 0.002),
            step_target: 1.0,
            brake_elapsed: None,
            paused_seek: false,
            playing: false,
            loop_range: None,
            slip: false,
            inserts: std::array::from_fn(|_| Effect::new(sr)),
            fx_clock: None,
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

    pub(super) fn capture_music_tail(&mut self, buffer: &AudioBuffer) {
        let source = crate::source::StemSource::new(
            buffer,
            self.stems.as_ref().map(|s| s.audio.as_ref()),
            self.stem_values,
        );
        let has_music = self.music.is_primed() && self.last_music_blend > 0.;
        if !has_music && !self.seek.active() {
            self.tail_index = self.transition_tail.len();
            return;
        }
        let mut position = self.position;
        for i in 0..self.transition_tail.len() {
            let mut step = self.step_target as f64;
            let (left, right) = if has_music && self.last_music_blend == 1. {
                (0., 0.)
            } else {
                self.resampler.stereo_at(&source, position, step)
            };
            let mut sample = [left, right];
            if has_music {
                let (music, music_step) =
                    self.music
                        .next(&source, &self.resampler, position, self.music_params);
                step = music_step;
                for ch in 0..2 {
                    sample[ch] += self.last_music_blend * (music[ch] - sample[ch]);
                }
            }
            if self.seek.active() {
                let g = self.seek.next_gain();
                let old = if self.tail_index < self.transition_tail.len() {
                    let old = self.transition_tail[self.tail_index];
                    self.tail_index += 1;
                    old
                } else {
                    let (l, r) = self.resampler.stereo_at(&source, self.old_ph, step);
                    [l, r]
                };
                for ch in 0..2 {
                    sample[ch] = old[ch] * (1. - g).sqrt() + sample[ch] * g.sqrt();
                }
                self.old_ph += step;
            }
            // The old tail is read ahead of the write cursor, so no second
            // buffer or callback allocation is needed for rapid retriggers.
            self.transition_tail[i] = sample;
            position += step;
        }
        self.tail_index = 0;
    }
}

impl AudioRt {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            cue_output: None,
            cue_limiter: MasterLimiter::new(sample_rate),
            decks: std::array::from_fn(|_| DeckRt::new(sample_rate)),
            master: SmoothValue::new(0.8, sample_rate, 0.005),
            master_gain: SmoothValue::new(1.0, sample_rate, 0.005),
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

impl Shared {
    pub(super) fn render_deck(
        &self,
        idx: usize,
        rt: &mut DeckRt,
        device_sr: f32,
        buf: Option<&AudioBuffer>,
    ) -> (f32, f32) {
        let slot = &self.decks[idx];
        rt.cue_sample = [0.0; 2];
        let Some(buf) = buf else {
            return (0.0, 0.0);
        };
        rt.stem_values = std::array::from_fn(|i| rt.stem_gain[i].next());
        let source = crate::source::StemSource::new(
            buf,
            rt.stems.as_ref().map(|s| s.audio.as_ref()),
            rt.stem_values,
        );

        let src_sr = buf.sample_rate.max(1) as f32;
        let mut step = rt.step.next() as f64;
        let roll_range = if slot.roll.load(Ordering::Relaxed) {
            let bpm = (slot.bpm_milli.load(Ordering::Relaxed) as f64 / 100.0).max(20.0);
            let start = slot.roll_start.load(Ordering::Relaxed) as f64 / 65536.0;
            let division = slot.roll_division.load(Ordering::Relaxed).max(1) as f64;
            let length = (60.0 / bpm) * (4.0 / division) * src_sr as f64;
            Some((start, (start + length).min(buf.frames as f64)))
                .filter(|(start, end)| end - start > 1.0)
        } else {
            None
        };
        let active_loop = roll_range.or(rt.loop_range);
        // Vinyl braking follows the device sample clock and bypasses key lock.
        // The tempo and key controls retain their settings for the next start.
        if let Some(elapsed) = rt.brake_elapsed.as_mut() {
            let time = *elapsed as f64 / (device_sr as f64 * 1.5);
            // Smooth onset followed by a progressively slower tail. Keep a
            // positive asymptote so an arbitrarily long hold can reach EOF.
            // Release controls the stop; elapsed time never stops the deck.
            let ratio = 0.03 + 0.97 / (1. + time * time);
            step *= ratio;
            if rt.playing {
                *elapsed = elapsed.saturating_add(1);
            }
        }
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
            1.
        } else {
            0.0
        };
        rt.amplitude.set(level);
        let amplitude = rt.amplitude.next();
        if !rt.playing && amplitude == 0. {
            rt.brake_elapsed = None;
        }
        let mut ph = rt.position;
        if !rt.touching {
            if let Some((start, end)) = active_loop {
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
        let music_blend = rt.music_blend.next();
        rt.last_music_blend = music_blend;
        let (mut l, mut r) = if amplitude > 0.0 && music_blend < 1. {
            rt.resampler.stereo_at(&source, ph, step)
        } else {
            (0.0, 0.0)
        };
        let fx_beat = rt
            .fx_clock
            .map(|(anchor, beat, slope)| beat + (ph - anchor) * slope);

        if amplitude > 0.0 && (rt.music_mode || music_blend > 0.0) {
            let (music, music_step) = rt.music.next(&source, &rt.resampler, ph, rt.music_params);
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
                rt.resampler.stereo_at(&source, rt.old_ph, step)
            };
            l = ol * fade_out + l * fade_in;
            r = or_ * fade_out + r * fade_in;
            rt.old_ph += step;
        }

        if rt.playing || rt.touching || (amplitude > 0.0 && !rt.paused_seek) {
            ph += step;
        }
        if rt.touching && rt.playing {
            rt.slip_position += rt.step_target as f64;
        }
        // The stored source position is clamped to the final sample. At a
        // fractional step, waiting for `frames` would strand playback there.
        if !rt.touching
            && active_loop.is_none()
            && (ph < 0.0 || ph >= buf.frames.saturating_sub(1) as f64)
        {
            rt.playing = false;
            slot.playing.store(false, Ordering::Relaxed);
            slot.brake.store(false, Ordering::Relaxed);
        }
        rt.position = if active_loop.is_some() && !rt.touching {
            ph
        } else {
            ph.clamp(0.0, buf.frames.saturating_sub(1) as f64)
        };
        rt.presentation_step = if rt.playing || rt.touching { step } else { 0. };

        let eq = std::array::from_fn(|band| rt.eq[band].next());
        rt.isolator_l.gain = eq;
        rt.isolator_r.gain = eq;

        let normalized_amplitude = amplitude * buf.loudness.gain;
        l = rt
            .filter_l
            .process(rt.isolator_l.process(l * normalized_amplitude));
        r = rt
            .filter_r
            .process(rt.isolator_r.process(r * normalized_amplitude));
        let mut stereo = [l, r];
        for effect in &mut rt.inserts {
            stereo = effect.process_clocked(stereo, false, fx_beat);
        }
        // TRIM calibrates the channel; GAIN independently drives its limiter.
        // Both master and PFL hear the same limited signal. Fader and balance
        // stay downstream so limiting cannot counteract a channel fade.
        // The two smoothers have different release times. Keep their product
        // within +12 dB even during a manual takeover of active compensation.
        let drive = (rt.limiter_gain.next() * rt.automix_gain.next()).min(3.981_071_7);
        let input_gain = rt.trim.next() * drive;
        let (left, right) = rt
            .limiter
            .process(stereo[0] * input_gain, stereo[1] * input_gain);
        rt.cue_sample = [left, right];
        let fader = rt.fader.next();
        l = left * fader * rt.balance_l.next();
        r = right * fader * rt.balance_r.next();
        rt.last_l = l;
        rt.last_r = r;
        (l, r)
    }
}
