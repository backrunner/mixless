//! Exact memoization within one immutable arrangement search. Different seeds
//! often descend through the same shapes; neither failures nor successes need
//! to be scored again. The iteration order and strict tie rules stay unchanged.
use super::{Frame, Shape, Tonality};
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    sync::Mutex,
};

pub(super) fn key(shape: &Shape) -> [u32; 16] {
    std::array::from_fn(|i| {
        let fade = shape[i / 8][i / 2 % 4];
        if i % 2 == 0 { fade.start } else { fade.end }.to_bits()
    })
}

pub(super) struct Costs<'a> {
    prepared: super::cost::Prepared<'a>,
    values: [Mutex<HashMap<[u32; 16], Option<f32>>>; 32],
    #[cfg(all(feature = "gpu-audit", target_os = "macos"))]
    capture: Option<crate::gpu_audit::Capture>,
}

impl<'a> Costs<'a> {
    pub(super) fn new(frames: &'a [Frame], tonality: Tonality) -> Self {
        let prepared = super::cost::Prepared::new(frames, tonality);
        Self {
            #[cfg(all(feature = "gpu-audit", target_os = "macos"))]
            capture: crate::gpu_audit::Capture::new(|| prepared.audit_input()),
            prepared,
            values: std::array::from_fn(|_| Mutex::new(HashMap::new())),
        }
    }

    pub(super) fn get(&self, shape: &Shape) -> Option<f32> {
        let result = self.lookup(shape);
        #[cfg(all(feature = "gpu-audit", target_os = "macos"))]
        self.observe(&[*shape], &[result]);
        result
    }

    /// Independent trials share one immutable frame set. CPU cache lookups
    /// and scoring retain their original order; only diagnostics run elsewhere.
    pub(super) fn batch<const N: usize>(&self, shapes: &[Shape; N]) -> [Option<f32>; N] {
        let results = std::array::from_fn(|i| self.lookup(&shapes[i]));
        #[cfg(all(feature = "gpu-audit", target_os = "macos"))]
        self.observe(shapes, &results);
        results
    }

    #[cfg(all(feature = "gpu-audit", target_os = "macos"))]
    fn observe(&self, shapes: &[Shape], scores: &[Option<f32>]) {
        if let Some(capture) = &self.capture {
            capture.observe(shapes.iter().map(|s| key(s).map(f32::from_bits)), scores);
        }
    }

    fn lookup(&self, shape: &Shape) -> Option<f32> {
        let key = key(shape);
        let mut hash = DefaultHasher::new();
        key.hash(&mut hash);
        let index = hash.finish() as usize % self.values.len();
        let mut values = self.values[index].lock().unwrap();
        if let Some(value) = values.get(&key) {
            #[cfg(all(feature = "gpu-audit", target_os = "macos"))]
            if let Some(capture) = &self.capture {
                capture.cache_hit();
            }
            return *value;
        }
        // `cost` is a leaf calculation: no other locks, evidence queries or
        // nested worker jobs. Keeping this shard locked coalesces identical
        // cold seeds without blocking unrelated shapes or risking pool starvation.
        #[cfg(all(feature = "gpu-audit", target_os = "macos"))]
        let started = self.capture.as_ref().map(|_| std::time::Instant::now());
        let value = self.prepared.score(shape);
        #[cfg(all(feature = "gpu-audit", target_os = "macos"))]
        if let (Some(started), Some(capture)) = (started, &self.capture) {
            capture.scored(started.elapsed());
        }
        if values.len() >= 128 {
            values.clear();
        }
        values.insert(key, value);
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
            let serial = Costs::new(&frames, tonality);
            let batched = Costs::new(&frames, tonality);
            for group in 0..1100 {
                let shapes: [Shape; 5] = std::array::from_fn(|i| {
                    [[Fade {
                        start: 4. + group as f32 / 10000.,
                        end: 6. + i as f32 / 4.,
                    }; 4]; 2]
                });
                let expected = shapes.map(|s| serial.get(&s).map(f32::to_bits));
                assert_eq!(
                    batched.batch(&shapes).map(|s| s.map(f32::to_bits)),
                    expected
                );
                assert_eq!(batched.get(&shapes[0]).map(f32::to_bits), expected[0]);
            }
            assert_eq!(batched.batch::<0>(&[]), []);
        }
    }
}
