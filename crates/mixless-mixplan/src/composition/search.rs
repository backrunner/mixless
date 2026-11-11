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
    frames: &'a [Frame],
    tonality: Tonality,
    values: [Mutex<HashMap<[u32; 16], Option<f32>>>; 32],
}

impl<'a> Costs<'a> {
    pub(super) fn new(frames: &'a [Frame], tonality: Tonality) -> Self {
        Self {
            frames,
            tonality,
            values: std::array::from_fn(|_| Mutex::new(HashMap::new())),
        }
    }

    pub(super) fn get(&self, shape: &Shape) -> Option<f32> {
        let key = key(shape);
        let mut hash = DefaultHasher::new();
        key.hash(&mut hash);
        let index = hash.finish() as usize % self.values.len();
        let mut values = self.values[index].lock().unwrap();
        if let Some(value) = values.get(&key) {
            return *value;
        }
        // `cost` is a leaf calculation: no other locks, evidence queries or
        // nested worker jobs. Keeping this shard locked coalesces identical
        // cold seeds without blocking unrelated shapes or risking pool starvation.
        let value = super::cost(self.frames, shape, self.tonality);
        if values.len() >= 128 {
            values.clear();
        }
        values.insert(key, value);
        value
    }
}
