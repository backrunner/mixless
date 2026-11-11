//! Exact memoization within one immutable arrangement search. Different seeds
//! often descend through the same shapes; neither failures nor successes need
//! to be scored again. The iteration order and strict tie rules stay unchanged.
use super::{Frame, Shape, Tonality};
use std::{cell::RefCell, collections::HashMap};

pub(super) fn key(shape: &Shape) -> [u32; 16] {
    std::array::from_fn(|i| {
        let fade = shape[i / 8][i / 2 % 4];
        if i % 2 == 0 { fade.start } else { fade.end }.to_bits()
    })
}

pub(super) struct Costs<'a> {
    frames: &'a [Frame],
    tonality: Tonality,
    values: RefCell<HashMap<[u32; 16], Option<f32>>>,
}

impl<'a> Costs<'a> {
    pub(super) fn new(frames: &'a [Frame], tonality: Tonality) -> Self {
        Self {
            frames,
            tonality,
            values: RefCell::new(HashMap::new()),
        }
    }

    pub(super) fn get(&self, shape: &Shape) -> Option<f32> {
        let key = key(shape);
        if let Some(value) = self.values.borrow().get(&key) {
            return *value;
        }
        let value = super::cost(self.frames, shape, self.tonality);
        let mut values = self.values.borrow_mut();
        // Bound memory even for unusually dense phrase evidence. Eviction only
        // causes recomputation, never truncation of the musical search.
        if values.len() >= 4096 {
            values.clear();
        }
        values.insert(key, value);
        value
    }
}
