//! Callback gesture lifecycle and bounded absolute navigation.
use super::*;

impl DeckRt {
    pub(super) fn update_scrub(&mut self, slot: &DeckSlot, touching: bool, sr: f32, xf_len: usize) {
        if self.playing {
            self.monitor_scrub = false;
        } else if touching {
            self.monitor_scrub = true;
        }
        if touching && !self.touching {
            self.slip_position = self.position;
            self.scratch_speed = 0.0;
        } else if !touching && self.touching {
            if self.slip && self.playing && !slot.jog_absolute.load(Ordering::Relaxed) {
                self.old_ph = self.position;
                self.tail_index = self.transition_tail.len();
                self.position = self.slip_position.clamp(
                    0.0,
                    slot.frames.load(Ordering::Relaxed).saturating_sub(1) as f64,
                );
                self.seek.start(xf_len);
            }
            // Don't carry a residual outward velocity past a committed track
            // boundary when normal playback would move back into the track.
            let target = if slot.seek_pending.load(Ordering::Acquire) {
                slot.seek_to.load(Ordering::Relaxed) as f64 / 65536.
            } else {
                self.position
            };
            let at_start = target <= 0. && self.step_target > 0.;
            let at_end = target >= slot.frames.load(Ordering::Relaxed).saturating_sub(1) as f64
                && self.step_target < 0.;
            let release_step = if slot.jog_absolute.load(Ordering::Relaxed) || at_start || at_end {
                self.step_target
            } else {
                self.scratch_speed as f32
            };
            self.step = SmoothValue::new(release_step, sr, 0.002);
            self.step.set(self.step_target);
            self.music.invalidate();
        }
        self.touching = touching;
        self.slip = slot.slip.load(Ordering::Relaxed);
    }

    pub(super) fn follow_scrub_target(
        &mut self,
        slot: &DeckSlot,
        buffer: Option<&AudioBuffer>,
        source_rate: f32,
        xf_len: usize,
    ) {
        // An absolute progress drag can cross minutes in one event. Audition
        // near the new target instead of chasing the intervening track at 8x.
        // Small moves use the same bidirectional scratch servo as the platter.
        if self.touching && slot.jog_absolute.load(Ordering::Relaxed) {
            let target = slot.jog_target.load(Ordering::Acquire) as f64 / 65536.;
            let distance = target - self.position;
            if distance.abs() > source_rate as f64 * 0.04 {
                if let Some(buffer) = buffer {
                    self.capture_music_tail(buffer);
                }
                self.music.invalidate();
                self.old_ph = self.position;
                self.position = (target - distance.signum() * source_rate as f64 * 0.012).clamp(
                    0.,
                    slot.frames.load(Ordering::Relaxed).saturating_sub(1) as f64,
                );
                self.seek.start(xf_len);
            }
        }
    }
}
