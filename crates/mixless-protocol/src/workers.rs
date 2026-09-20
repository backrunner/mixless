//! Shared limits for offline work; keep headroom for audio and UI threads.
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

// All pools use the same hardware snapshot, including lazily loaded models.
// A failed detection must not assume a multicore machine.
fn hardware() -> &'static (BackgroundWorkers, CpuBudget) {
    static HARDWARE: OnceLock<(BackgroundWorkers, CpuBudget)> = OnceLock::new();
    HARDWARE.get_or_init(|| {
        let cpus = std::thread::available_parallelism().map_or(1, usize::from);
        (
            BackgroundWorkers::for_cpus(cpus),
            CpuBudget::new(cpus.saturating_sub(2).max(1)),
        )
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackgroundWorkers {
    pub analysis: usize,
    pub stems: usize,
    pub inference_threads: usize,
}

impl BackgroundWorkers {
    pub fn detected() -> Self {
        hardware().0
    }

    /// Reserve CPU capacity across analysis and inference pools. On small CPUs
    /// the two kinds of work take turns; model waiters remain cancellable.
    pub fn acquire_cpu(
        threads: usize,
        active: impl Fn() -> bool,
    ) -> Option<BackgroundCpuPermit<'static>> {
        hardware().1.acquire(threads, active)
    }

    pub fn for_cpus(cpus: usize) -> Self {
        let available = cpus.saturating_sub(2).max(1);
        // Full-track PCM and model activations are large. Scale CPU work while
        // keeping at most two model sets and eight basic analyses resident.
        let stems = if available >= 6 { 2 } else { 1 };
        let inference_threads = (available / 2 / stems).clamp(1, 4);
        let analysis = available
            .saturating_sub(stems * inference_threads)
            .clamp(1, 8);
        Self {
            analysis,
            stems,
            inference_threads,
        }
    }
}

struct CpuBudget {
    capacity: usize,
    available: Mutex<usize>,
    wake: Condvar,
}

/// Releases its share of the process-wide background CPU budget on drop.
pub struct BackgroundCpuPermit<'a> {
    budget: &'a CpuBudget,
    threads: usize,
}

impl CpuBudget {
    fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            available: Mutex::new(capacity),
            wake: Condvar::new(),
        }
    }

    fn acquire(
        &self,
        threads: usize,
        active: impl Fn() -> bool,
    ) -> Option<BackgroundCpuPermit<'_>> {
        let threads = threads.clamp(1, self.capacity);
        let mut available = self.available.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if !active() {
                return None;
            }
            if *available >= threads {
                *available -= threads;
                return Some(BackgroundCpuPermit {
                    budget: self,
                    threads,
                });
            }
            available = self
                .wake
                .wait_timeout(available, Duration::from_millis(25))
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

impl Drop for BackgroundCpuPermit<'_> {
    fn drop(&mut self) {
        *self
            .budget
            .available
            .lock()
            .unwrap_or_else(|p| p.into_inner()) += self.threads;
        // Waiters can request different numbers of threads.
        self.budget.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_pools_share_a_cpu_budget_and_bound_memory() {
        for cpus in 0..=128 {
            let budget = BackgroundWorkers::for_cpus(cpus);
            assert!((1..=8).contains(&budget.analysis));
            assert!((1..=2).contains(&budget.stems));
            assert!((1..=4).contains(&budget.inference_threads));
            assert!(
                budget.analysis + budget.stems * budget.inference_threads
                    <= cpus.saturating_sub(2).max(2)
            );
        }
        assert_eq!(
            BackgroundWorkers::for_cpus(10),
            BackgroundWorkers {
                analysis: 4,
                stems: 2,
                inference_threads: 2,
            }
        );
    }

    #[test]
    fn low_core_machine_serializes_analysis_and_stems_and_releases_capacity() {
        let budget = CpuBudget::new(1);
        let analysis = budget.acquire(1, || true).unwrap();
        std::thread::scope(|scope| {
            let (tx, rx) = std::sync::mpsc::channel();
            let budget = &budget;
            scope.spawn(move || {
                let _stems = budget.acquire(1, || true).unwrap();
                tx.send(()).unwrap();
            });
            assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
            drop(analysis);
            rx.recv_timeout(Duration::from_secs(2)).unwrap();
        });
        assert!(budget.acquire(1, || true).is_some());
    }

    #[test]
    fn weighted_inference_wait_can_cancel_without_consuming_capacity() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let budget = CpuBudget::new(3);
        let analysis = budget.acquire(1, || true).unwrap();
        let active = AtomicBool::new(true);
        std::thread::scope(|scope| {
            let (tx, rx) = std::sync::mpsc::channel();
            let (budget, active) = (&budget, &active);
            scope.spawn(move || {
                tx.send(
                    budget
                        .acquire(3, || active.load(Ordering::Acquire))
                        .is_none(),
                )
                .unwrap();
            });
            assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
            active.store(false, Ordering::Release);
            assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap());
        });
        let stems = budget.acquire(2, || true).unwrap();
        assert_eq!(*budget.available.lock().unwrap(), 0);
        drop((analysis, stems));
        assert_eq!(*budget.available.lock().unwrap(), 3);
    }
}
