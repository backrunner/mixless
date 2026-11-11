//! Compare cold/warm pair scores while still running the complete chain repair.
//! Uses an immutable export and memory only; never opens the live library.
use mixless_mixplan::{
    order::{best_cycle_while_cached, PairCostCache},
    PlannerOptions,
};
use mixless_protocol::TrackAnalysis;
use std::{
    collections::HashMap,
    error::Error,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::Instant,
};

#[derive(Default)]
struct Cache {
    values: Mutex<HashMap<(usize, usize), f32>>,
    hits: AtomicUsize,
    writes: AtomicUsize,
}
impl PairCostCache for Cache {
    fn get(&self, a: usize, b: usize) -> Option<f32> {
        let value = self.values.lock().unwrap().get(&(a, b)).copied();
        if value.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
        value
    }
    fn store(&self, a: usize, b: usize, cost: f32) {
        self.values.lock().unwrap().insert((a, b), cost);
        self.writes.fetch_add(1, Ordering::Relaxed);
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(
        args.first().ok_or("expected analyses.json [--stems]")?,
    )?)?;
    let refs: Vec<_> = tracks.iter().collect();
    let n = tracks.len();
    let pairs = if n > 200 {
        0
    } else {
        n * if n <= 40 { n.saturating_sub(1) } else { 12 }
    };
    let cache = Cache::default();
    let options = PlannerOptions {
        harmonic_key_shift: true,
        stem_playback: args.iter().any(|a| a == "--stems"),
        ..Default::default()
    };
    let mut previous = None;
    for state in ["cold", "warm"] {
        let start = Instant::now();
        let pair_end = Mutex::new(None);
        let order = best_cycle_while_cached(
            &refs,
            &options,
            |done, _| {
                if done >= n + pairs {
                    pair_end
                        .lock()
                        .unwrap()
                        .get_or_insert_with(|| start.elapsed().as_secs_f64());
                }
            },
            || true,
            Some(&cache),
        )
        .unwrap();
        let seconds = start.elapsed().as_secs_f64();
        if let Some(old) = previous {
            assert_eq!(order, old, "cache changed the chosen order");
        }
        println!(
            "{}",
            serde_json::json!({"state":state, "tracks":n, "seconds":seconds, "pair_phase_seconds":pair_end.into_inner().unwrap(), "hits":cache.hits.swap(0, Ordering::Relaxed), "measured":cache.writes.swap(0, Ordering::Relaxed), "order":order})
        );
        previous = Some(order);
    }
    Ok(())
}
