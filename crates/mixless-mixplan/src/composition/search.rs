//! Exact memoization within one immutable arrangement search. Different seeds
//! often descend through the same shapes; neither failures nor successes need
//! to be scored again. The iteration order and strict tie rules stay unchanged.
//! Drum variants may share a failed vocal/tonal/foreground gate; level failures
//! and successful scores always retain the complete shape key.
use super::{Frame, Shape, Tonality};
use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::Mutex,
};

pub(super) fn key(shape: &Shape) -> [u32; 16] {
    std::array::from_fn(|i| {
        let fade = shape[i / 8][i / 2 % 4];
        if i % 2 == 0 { fade.start } else { fade.end }.to_bits()
    })
}

#[derive(Default)]
struct Cache {
    values: HashMap<[u32; 16], Option<f32>>,
    rejected: HashSet<[u32; 12]>,
}

pub(super) struct Costs<'a> {
    prepared: super::cost::Prepared<'a>,
    values: [Mutex<Cache>; 32],
}

impl<'a> Costs<'a> {
    pub(super) fn new(frames: &'a [Frame], tonality: Tonality) -> Self {
        let prepared = super::cost::Prepared::new(frames, tonality);
        Self {
            prepared,
            values: std::array::from_fn(|_| Mutex::new(Cache::default())),
        }
    }

    pub(super) fn get(&self, shape: &Shape) -> Option<f32> {
        self.lookup(shape)
    }

    /// Independent trials share one immutable frame set. CPU cache lookups
    /// and scoring retain their original order.
    pub(super) fn batch<const N: usize>(&self, shapes: &[Shape; N]) -> [Option<f32>; N] {
        std::array::from_fn(|i| self.lookup(&shapes[i]))
    }

    fn lookup(&self, shape: &Shape) -> Option<f32> {
        let key = key(shape);
        // Only drum fades (part 1) are omitted. Every input to the shared
        // rejection gates, including both vocal/melodic/bass fades, stays exact.
        let shared: [u32; 12] =
            std::array::from_fn(|i| key[[0, 1, 4, 5, 6, 7, 8, 9, 12, 13, 14, 15][i]]);
        let mut hash = DefaultHasher::new();
        shared.hash(&mut hash);
        let index = hash.finish() as usize % self.values.len();
        let mut cache = self.values[index].lock().unwrap();
        if let Some(value) = if cache.rejected.contains(&shared) {
            Some(None)
        } else {
            cache.values.get(&key).copied()
        } {
            return value;
        }
        // `cost` is a leaf calculation: no other locks, evidence queries or
        // nested worker jobs. Keeping this shard locked coalesces identical
        // cold seeds without blocking unrelated shapes or risking pool starvation.
        let value = self.prepared.score(shape);
        let shared_rejection = matches!(value, Err(super::cost::Rejection::Shared));
        let value = value.ok();
        if shared_rejection {
            if cache.rejected.len() >= 128 {
                cache.rejected.clear();
            }
            cache.rejected.insert(shared);
        } else {
            if cache.values.len() >= 128 {
                cache.values.clear();
            }
            cache.values.insert(key, value);
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::Fade;
    #[test]
    fn batches_preserve_ordered_scores_cache_eviction_and_ties() {
        let frames: Vec<_> = (0..129)
            .map(|i| Frame {
                u: i as f32 / 8.,
                sec: i as f32 / 4.,
                bands: [[[0.01; 3]; 4]; 2],
                voice: [0., 0.],
            })
            .collect();
        for tonality in [Tonality::Compatible, Tonality::Unknown, Tonality::Conflict] {
            let serial = super::super::cost::Prepared::new(&frames, tonality);
            let batched = Costs::new(&frames, tonality);
            for group in 0..1100 {
                let shapes: [Shape; 5] = std::array::from_fn(|i| {
                    [[Fade {
                        start: 4. + group as f32 / 10000.,
                        end: 6. + i as f32 / 4.,
                    }; 4]; 2]
                });
                let expected = shapes.map(|s| serial.score(&s).ok().map(f32::to_bits));
                assert_eq!(
                    batched.batch(&shapes).map(|s| s.map(f32::to_bits)),
                    expected
                );
                assert_eq!(batched.get(&shapes[0]).map(f32::to_bits), expected[0]);
            }
            assert_eq!(batched.batch::<0>(&[]), []);
            for shard in &batched.values {
                let cache = shard.lock().unwrap();
                assert!(cache.values.len() <= 128 && cache.rejected.len() <= 128);
            }
        }
    }

    #[test]
    fn a_power_loss_cannot_poison_other_drum_widths() {
        let frames: Vec<_> = (0..129)
            .map(|i| Frame {
                u: i as f32 / 8.,
                sec: i as f32 / 4.,
                bands: [[[0.001; 3], [1.; 3], [0.001; 3], [0.001; 3]]; 2],
                voice: [0., 0.],
            })
            .collect();
        let prepared = super::super::cost::Prepared::new(&frames, Tonality::Compatible);
        let costs = Costs::new(&frames, Tonality::Compatible);
        let good = [[Fade {
            start: 20.,
            end: 21.,
        }; 4]; 2];
        let mut bad = good;
        bad[0][1] = Fade {
            start: -2.,
            end: -1.,
        };
        assert!(matches!(
            prepared.score(&bad),
            Err(super::super::cost::Rejection::Level)
        ));
        assert!(prepared.score(&good).is_ok());
        assert_eq!(costs.get(&bad), None);
        assert_eq!(
            costs.get(&good).map(f32::to_bits),
            prepared.score(&good).ok().map(f32::to_bits)
        );
        assert_eq!(costs.get(&bad), None);
    }

    #[test]
    fn shared_rejections_keep_the_other_six_fades_in_the_key() {
        let frames: Vec<_> = (0..129)
            .map(|i| Frame {
                u: i as f32 / 8.,
                sec: i as f32 / 4.,
                bands: [[[0.01; 3]; 4]; 2],
                voice: [0.9, 0.9],
            })
            .collect();
        let costs = Costs::new(&frames, Tonality::Compatible);
        let good = [[Fade {
            start: 20.,
            end: 21.,
        }; 4]; 2];
        let mut bad = good;
        bad[1][0] = Fade {
            start: -2.,
            end: -1.,
        };
        for width in [0.5, 1., 2., 4.] {
            bad[0][1].end = bad[0][1].start + width;
            assert_eq!(costs.get(&bad), None);
        }
        assert_eq!(
            costs
                .values
                .iter()
                .map(|s| s.lock().unwrap().rejected.len())
                .sum::<usize>(),
            1
        );
        // Restoring only the voice fade makes the arrangement valid, even
        // though the previous rejected drum variants remain cached.
        assert!(costs.get(&good).is_some());
    }
}
