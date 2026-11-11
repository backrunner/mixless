//! Disk I/O stays outside Rayon jobs; workers only read an immutable matrix
//! and append newly measured scores. Chain corrections are never persisted.
use mixless_library::Library;
use mixless_mixplan::order::PairCostCache;
use sha2::{Digest, Sha256};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

pub(super) struct Pairs {
    n: usize,
    entries: Vec<Option<(String, Option<f32>)>>,
    learned: Mutex<Vec<(String, f32)>>,
    hits: AtomicUsize,
}

impl Pairs {
    pub(super) fn load(
        library: &Library,
        tracks: &[crate::analysis::PreparedTrack],
        options: &mixless_mixplan::PlannerOptions,
    ) -> Option<Self> {
        // Bound the prefetch too. Larger sets currently use only rough costs.
        if !(3..=200).contains(&tracks.len()) {
            return None;
        }
        let fingerprints = super::order_cache::track_keys(tracks, options).ok()?;
        Self::load_keys(library, &fingerprints)
            .map_err(|error| {
                tracing::warn!(%error, "Best order pair cache read failed");
            })
            .ok()
    }

    fn load_keys(
        library: &Library,
        fingerprints: &[String],
    ) -> Result<Self, mixless_library::LibraryError> {
        let n = fingerprints.len();
        let mut keys = Vec::with_capacity(n * n.saturating_sub(1));
        let mut positions = Vec::with_capacity(keys.capacity());
        for (from, a) in fingerprints.iter().enumerate() {
            for (to, b) in fingerprints.iter().enumerate() {
                if from == to {
                    continue;
                }
                positions.push(from * n + to);
                keys.push(pair_key(a, b));
            }
        }
        let values = library.load_pair_costs(&keys)?;
        let mut entries = vec![None; n * n];
        for ((position, key), value) in positions.into_iter().zip(keys).zip(values) {
            entries[position] = Some((key, value));
        }
        Ok(Self {
            n,
            entries,
            learned: Mutex::default(),
            hits: AtomicUsize::new(0),
        })
    }

    pub(super) fn save(&self, library: &Library) {
        let learned = self.learned.lock().expect("pair cache");
        if !learned.is_empty()
            && let Err(error) = library.save_pair_costs(&learned)
        {
            tracing::warn!(%error, "Best order pair cache write failed");
        }
        tracing::debug!(
            hits = self.hits.load(Ordering::Relaxed),
            measured = learned.len(),
            "Best order pair cache"
        );
    }
}

impl PairCostCache for Pairs {
    fn get(&self, from: usize, to: usize) -> Option<f32> {
        let value = self.entries[from * self.n + to].as_ref()?.1?;
        self.hits.fetch_add(1, Ordering::Relaxed);
        Some(value)
    }
    fn store(&self, from: usize, to: usize, cost: f32) {
        if let Some((key, _)) = &self.entries[from * self.n + to] {
            self.learned
                .lock()
                .expect("pair cache")
                .push((key.clone(), cost));
        }
    }
}

fn pair_key(from: &str, to: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"mixless-cold-pair-v1");
    hash.update(from.as_bytes());
    hash.update(to.as_bytes());
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_survive_reordering_insertion_and_restart_without_reusing_changed_tracks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let library = Library::open(&path).unwrap();
        let original = ["a", "b", "c"].map(str::to_owned);
        let cache = Pairs::load_keys(&library, &original).unwrap();
        cache.store(0, 1, 0.25);
        cache.store(1, 0, 0.75);
        assert_eq!(cache.get(0, 1), None); // Only the saved snapshot is shared.
        cache.save(&library);
        drop(library);
        let library = Library::open(&path).unwrap();
        let changed = ["new", "b", "a", "c-changed"].map(str::to_owned);
        let cache = Pairs::load_keys(&library, &changed).unwrap();
        assert_eq!(cache.get(2, 1), Some(0.25));
        assert_eq!(cache.get(1, 2), Some(0.75));
        assert_eq!(cache.get(0, 1), None);
        assert_eq!(cache.get(2, 3), None);
        assert_eq!(cache.get(2, 2), None);
        cache.store(0, 1, 1.);
        drop(cache); // A cancelled job does not flush newly measured scores.
        assert_eq!(
            Pairs::load_keys(&library, &changed).unwrap().get(0, 1),
            None
        );
    }
}
