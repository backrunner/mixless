/// Host-owned cache for cold pair costs. The host must key both immutable
/// analyses, source identities, planner options and ORDER_VERSION. Carried
/// chain corrections never enter this cache.
pub trait PairCostCache: Sync {
    fn get(&self, from: usize, to: usize) -> Option<f32>;
    fn store(&self, from: usize, to: usize, cost: f32);
}

#[cfg(all(test, not(feature = "trace")))]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    #[derive(Default)]
    struct Cache {
        costs: Mutex<std::collections::HashMap<(usize, usize), f32>>,
        hits: AtomicUsize,
        writes: AtomicUsize,
    }
    impl PairCostCache for Cache {
        fn get(&self, from: usize, to: usize) -> Option<f32> {
            let cost = self.costs.lock().unwrap().get(&(from, to)).copied();
            if cost.is_some() {
                self.hits.fetch_add(1, Ordering::Relaxed);
            }
            cost
        }
        fn store(&self, from: usize, to: usize, cost: f32) {
            self.costs.lock().unwrap().insert((from, to), cost);
            self.writes.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn warm_pairs_preserve_order_and_progress_and_invalid_scores_are_remeasured() {
        let tracks: Vec<_> = [120., 128., 174.]
            .iter()
            .enumerate()
            .map(|(i, bpm)| {
                crate::tests::track(
                    i as i64,
                    *bpm,
                    "5A",
                    mixless_protocol::SectionLabel::Drop,
                    32,
                    0.9,
                    0.2,
                )
            })
            .collect();
        let refs: Vec<_> = tracks.iter().collect();
        let options = crate::PlannerOptions::default();
        let cache = Cache::default();
        let run = || {
            let reports = Mutex::new(Vec::new());
            let order = crate::order::best_cycle_while_cached(
                &refs,
                &options,
                |d, t| {
                    reports.lock().unwrap().push((d, t));
                },
                || true,
                Some(&cache),
            )
            .unwrap();
            let mut reports = reports.into_inner().unwrap();
            let total = reports[0].1;
            reports.sort_unstable();
            assert_eq!(reports, (0..=total).map(|d| (d, total)).collect::<Vec<_>>());
            order
        };
        let cold = run();
        assert_eq!(cache.writes.load(Ordering::Relaxed), 6);
        assert_eq!(run(), cold);
        assert_eq!(cache.hits.load(Ordering::Relaxed), 6);
        assert_eq!(cache.writes.load(Ordering::Relaxed), 6);
        cache.costs.lock().unwrap().insert((0, 1), f32::NAN);
        cache.costs.lock().unwrap().insert((1, 0), -1.);
        assert_eq!(run(), cold);
        assert_eq!(cache.writes.load(Ordering::Relaxed), 8);
        assert!(crate::order::best_cycle_while_cached(
            &refs,
            &options,
            |_, _| panic!("cancelled"),
            || false,
            Some(&cache)
        )
        .is_none());
        assert_eq!(cache.hits.load(Ordering::Relaxed), 12);
    }
}
