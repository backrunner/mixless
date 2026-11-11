//! An estimated position is usable only after proving the exact lower bound.
//! Irregular timing and rounding misses retain the original partition search.
use mixless_protocol::StemFrame;

pub(super) struct Index {
    ends: Vec<f32>,
    estimate: Option<(f32, f32)>,
}

impl Index {
    pub(super) fn new(frames: &[StemFrame]) -> Self {
        let ends: Vec<_> = frames.iter().map(|f| f.end_sec).collect();
        let estimate = frames.first().zip(frames.last()).and_then(|(first, last)| {
            let span = last.end_sec - first.start_sec;
            let density = frames.len() as f32 / span;
            (first.start_sec.is_finite()
                && span > 0.
                && density.is_finite()
                && ends.iter().all(|end| end.is_finite())
                && ends.windows(2).all(|w| w[0] <= w[1]))
            .then_some((first.start_sec, density))
        });
        Self { ends, estimate }
    }

    pub(super) fn at(&self, sec: f32) -> usize {
        if let Some((origin, density)) = self.estimate {
            if sec.is_finite() {
                let i = (((sec - origin) * density) as usize).min(self.ends.len() - 1);
                // For ordered ends these two comparisons prove that i is the
                // first end strictly after sec, even with duplicate endpoints.
                if (i == 0 || self.ends[i - 1] <= sec) && sec < self.ends[i] {
                    return i;
                }
                // A partial final frame or float rounding often moves the
                // estimate by one. These brackets prove those neighbours too.
                if self.ends[i] <= sec && (i + 1 == self.ends.len() || sec < self.ends[i + 1]) {
                    return i + 1;
                }
                if i > 0 && sec < self.ends[i - 1] && (i == 1 || self.ends[i - 2] <= sec) {
                    return i - 1;
                }
            }
        }
        self.ends.partition_point(|end| *end <= sec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(start_sec: f32, end_sec: f32) -> StemFrame {
        StemFrame {
            start_sec,
            end_sec,
            rms: [0.; 3],
            band_db: [[0.; 3]; 3],
            onset: [0.; 3],
            drum_low_onset: 0.,
            vocal_activity: 0.,
            note_chroma: [0.; 12],
        }
    }

    fn compare(frames: &[StemFrame]) {
        let index = Index::new(frames);
        let mut times = vec![f32::NEG_INFINITY, -1., -0., 0., f32::INFINITY, f32::NAN];
        for f in frames {
            for sec in [f.start_sec, f.end_sec] {
                times.extend([sec.next_down(), sec, sec.next_up()]);
            }
        }
        times.extend((0..4096).map(|i| i as f32 * 0.03125 - 4.));
        for sec in times {
            assert_eq!(
                index.at(sec),
                frames.partition_point(|f| f.end_sec <= sec),
                "source time {sec:?}"
            );
        }
    }

    #[test]
    fn estimated_positions_match_partition_at_boundaries_gaps_and_changed_clocks() {
        compare(&[]);
        for count in [1, 5, 97, 1001] {
            for step in [0.0375, 0.1, 0.125, 0.14] {
                for shift in [-10., 0., 12.] {
                    let mut frames: Vec<_> = (0..count)
                        .map(|i| frame(shift + i as f32 * step, shift + (i + 1) as f32 * step))
                        .collect();
                    compare(&frames);
                    // A short final frame, missing observations and variable
                    // lengths must never be approximated by the estimated clock.
                    frames.last_mut().unwrap().end_sec -= step * 0.4;
                    frames = frames
                        .into_iter()
                        .enumerate()
                        .filter_map(|(i, f)| (i % 7 != 3).then_some(f))
                        .collect();
                    for (i, f) in frames.iter_mut().enumerate() {
                        f.end_sec -= step * (i % 3) as f32 * 0.1;
                    }
                    compare(&frames);
                }
            }
        }
    }

    #[test]
    fn duplicate_and_invalid_endpoints_keep_the_original_lookup() {
        let mut frames = vec![frame(0., 1.), frame(0.5, 1.), frame(1., 2.), frame(2., 4.)];
        compare(&frames);
        frames.swap(0, 3);
        compare(&frames);
        frames[1].end_sec = f32::NAN;
        compare(&frames);
        frames[0].start_sec = f32::NEG_INFINITY;
        compare(&frames);
    }
}
