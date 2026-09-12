//! Audio-clock anchors for display interpolation. No UI-owned transport clock.
use super::*;
use std::time::{Duration, Instant};

pub(super) struct PresentationClock {
    origin: Instant,
    sequence: AtomicU64,
    time: AtomicU64,
    period: AtomicU64,
    tracks: [AtomicU64; 2],
    frames: [AtomicU64; 2],
    speeds: [AtomicU64; 2],
}

impl Default for PresentationClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            sequence: AtomicU64::new(0),
            time: AtomicU64::new(0),
            period: AtomicU64::new(0),
            tracks: std::array::from_fn(|_| AtomicU64::new(0)),
            frames: std::array::from_fn(|_| AtomicU64::new(0)),
            speeds: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl Shared {
    pub(super) fn publish_presentation(&self, rt: &AudioRt, end: Instant, period: Duration) {
        let clock = &self.presentation;
        clock.sequence.fetch_add(1, Ordering::AcqRel);
        clock.time.store(
            end.saturating_duration_since(clock.origin).as_nanos() as u64,
            Ordering::Relaxed,
        );
        clock
            .period
            .store(period.as_nanos() as u64, Ordering::Relaxed);
        for i in 0..2 {
            clock.tracks[i].store(
                self.decks[i].track_id.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            clock.frames[i].store(rt.decks[i].position.to_bits(), Ordering::Relaxed);
            clock.speeds[i].store(
                (rt.decks[i].presentation_step * self.sample_rate.load(Ordering::Relaxed) as f64)
                    .to_bits(),
                Ordering::Relaxed,
            );
        }
        clock.sequence.fetch_add(1, Ordering::Release);
    }
}

fn project(frame: f64, speed: f64, delta: f64, deck: &mixless_protocol::DeckSnapshot) -> f64 {
    let frame = frame + speed * delta;
    if deck.loop_on && deck.loop_end_frame > deck.loop_start_frame {
        let start = deck.loop_start_frame as f64;
        let length = (deck.loop_end_frame - deck.loop_start_frame) as f64;
        start + (frame - start).rem_euclid(length)
    } else {
        frame.clamp(0., deck.frames as f64)
    }
}

impl Engine {
    /// Interpolate against the last device-timestamped audio block. A stalled
    /// device, pause, pending seek or changed track cannot make the display run on.
    pub fn presentation_frames(&self, snapshot: &EngineSnapshot) -> [f64; 2] {
        let clock = &self.shared.presentation;
        let fallback = snapshot.decks.each_ref().map(|d| d.frame as f64);
        for _ in 0..3 {
            let seq = clock.sequence.load(Ordering::Acquire);
            if seq == 0 || seq & 1 != 0 {
                continue;
            }
            let time = clock.time.load(Ordering::Relaxed);
            let period = clock.period.load(Ordering::Relaxed);
            let tracks = clock.tracks.each_ref().map(|v| v.load(Ordering::Relaxed));
            let frames = clock
                .frames
                .each_ref()
                .map(|v| f64::from_bits(v.load(Ordering::Relaxed)));
            let speeds = clock
                .speeds
                .each_ref()
                .map(|v| f64::from_bits(v.load(Ordering::Relaxed)));
            std::sync::atomic::fence(Ordering::Acquire);
            if clock.sequence.load(Ordering::Relaxed) != seq {
                continue;
            }
            let delta = (clock.origin.elapsed().as_nanos() as f64 - time as f64) * 1e-9;
            let limit = (period as f64 * 2e-9).clamp(0.01, 0.1);
            return std::array::from_fn(|i| {
                let d = &snapshot.decks[i];
                if !d.playing
                    || d.roll
                    || d.brake
                    || self.shared.decks[i].jog_touch.load(Ordering::Acquire)
                    || self.shared.decks[i].seek_pending.load(Ordering::Acquire)
                    || tracks[i] != d.track_id.map_or(0, |id| id.0 as u64)
                    || delta > limit
                    || delta < -0.5
                    || !frames[i].is_finite()
                    || !speeds[i].is_finite()
                {
                    fallback[i]
                } else {
                    project(frames[i], speeds[i], delta, d)
                }
            });
        }
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_frames_advance_between_audio_blocks_and_wrap_fractional_loops() {
        let mut deck = mixless_protocol::DeckSnapshot::default();
        deck.frames = 480_000;
        let frames: Vec<_> = (0..4)
            .map(|i| project(512., 48_000., -512. / 48_000. + i as f64 / 240., &deck))
            .collect();
        for pair in frames.windows(2) {
            assert!((pair[1] - pair[0] - 200.).abs() < 1e-6);
        }
        deck.loop_on = true;
        deck.loop_start_frame = 1000;
        deck.loop_end_frame = 2500;
        assert_eq!(project(2400., 48_000., 0.01, &deck), 1380.);
        assert_eq!(project(1100., -48_000., 0.01, &deck), 2120.);
    }
}
