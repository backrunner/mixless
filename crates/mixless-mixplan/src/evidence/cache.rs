//! Bounded source evidence. A shared miss never holds a lock while measuring
//! audio or entering nested Rayon work; duplicate cold work is harmless.
use mixless_protocol::TrackAnalysis;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

type Key = (usize, usize);
const SHARDS: usize = 32;
const SHARD_ENTRIES: usize = 4096;
const LOCAL_ENTRIES: usize = 4096;

pub(crate) struct Shared<V> {
    shards: [Mutex<HashMap<Key, V>>; SHARDS],
}
impl<V> Default for Shared<V> {
    fn default() -> Self {
        Self {
            shards: std::array::from_fn(|_| Mutex::new(HashMap::new())),
        }
    }
}
impl<V: Copy> Shared<V> {
    fn shard(&self, key: Key) -> &Mutex<HashMap<Key, V>> {
        // Mix the float/frame bits too: adjacent float times often share low bits.
        let hash = key.0.rotate_left(13) ^ key.1.wrapping_mul(0x9e3779b1);
        &self.shards[(hash ^ (hash >> 16)) % SHARDS]
    }
    fn get(&self, key: Key) -> Option<V> {
        self.shard(key).lock().unwrap().get(&key).copied()
    }
    fn insert(&self, key: Key, value: V) {
        let mut shard = self.shard(key).lock().unwrap();
        if shard.len() >= SHARD_ENTRIES && !shard.contains_key(&key) {
            shard.clear();
        }
        shard.insert(key, value);
    }
}

pub(crate) struct Local<V> {
    tracks: [usize; 2],
    values: HashMap<Key, V>,
    shared: Arc<Shared<V>>,
}
impl<V: Copy> Local<V> {
    pub(crate) fn new(a: &TrackAnalysis, b: &TrackAnalysis, shared: Arc<Shared<V>>) -> Self {
        Self {
            tracks: [a as *const _ as usize, b as *const _ as usize],
            values: HashMap::new(),
            shared,
        }
    }
    pub(crate) fn shared(&self, a: &TrackAnalysis, b: &TrackAnalysis) -> Option<Arc<Shared<V>>> {
        [a, b]
            .iter()
            .all(|t| self.tracks.contains(&(*t as *const _ as usize)))
            .then(|| Arc::clone(&self.shared))
    }
    pub(crate) fn get(&mut self, key: Key) -> Option<V> {
        if !self.tracks.contains(&key.0) {
            return None;
        }
        if let Some(v) = self.values.get(&key) {
            return Some(*v);
        }
        let value = self.shared.get(key)?;
        self.remember(key, value);
        Some(value)
    }
    pub(crate) fn insert(&mut self, key: Key, value: V) {
        if self.tracks.contains(&key.0) {
            self.shared.insert(key, value);
            self.remember(key, value);
        }
    }
    fn remember(&mut self, key: Key, value: V) {
        if self.values.len() >= LOCAL_ENTRIES {
            self.values.clear();
        }
        self.values.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn misses_and_eviction_do_not_turn_absence_into_evidence() {
        let shared = Shared::<Option<u32>>::default();
        shared.insert((1, 1), None);
        assert_eq!(shared.get((1, 1)), Some(None));
        for i in 0..SHARDS * (SHARD_ENTRIES + 1) {
            shared.insert((1, i), Some(i as u32));
        }
        assert!(shared
            .shards
            .iter()
            .all(|s| s.lock().unwrap().len() <= SHARD_ENTRIES));
        for i in 0..SHARDS * (SHARD_ENTRIES + 1) {
            if let Some(v) = shared.get((1, i)) {
                assert_eq!(v, Some(i as u32));
            }
        }
    }
}
