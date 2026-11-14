//! Separate bounded pools for playback deadlines and background forecasts.
//! Nested requests stay on their caller's pool and retain exact search order.
use rayon::prelude::*;
use std::cell::Cell;
use std::sync::OnceLock;

thread_local! { static PLAYBACK: Cell<bool> = const { Cell::new(false) }; }

struct Scope(bool);
impl Scope {
    fn enter(playback: bool) -> Self {
        Self(PLAYBACK.replace(playback))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        PLAYBACK.set(self.0);
    }
}

/// Run host-side planning needed for the playing deck on reserved workers.
/// Background Best/preview jobs cannot occupy these workers. Nested searches
/// retain this scope; candidate budgets, scoring and tie order are unchanged.
/// This still performs expensive work and must never run on an audio callback.
pub fn with_playback_workers<T>(f: impl FnOnce() -> T) -> T {
    let _scope = Scope::enter(true);
    f()
}

fn pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    static PLAYBACK_POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    let playback = PLAYBACK.get();
    let pool = if playback { &PLAYBACK_POOL } else { &POOL };
    pool.get_or_init(|| {
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
        rayon::ThreadPoolBuilder::new()
            // Keep the existing background search capacity. Only a small
            // reserve can run alongside it when playback needs a plan now.
            .num_threads(if playback { workers.min(4) } else { workers })
            .thread_name(move |i| {
                if playback {
                    format!("mixless-playback-plan-{i}")
                } else {
                    format!("mixless-plan-{i}")
                }
            })
            .build()
            .ok()
    })
    .as_ref()
}

pub(crate) fn map<T: Send>(count: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    if count <= 1 {
        return (0..count).map(f).collect();
    }
    let Some(pool) = pool() else {
        return (0..count).map(f).collect();
    };
    let scheduling = crate::scheduling::Scheduling::current();
    let playback = PLAYBACK.get();
    let f = &f;
    crate::memo::with_workers(|memo| {
        pool.install(|| {
            (0..count)
                .into_par_iter()
                .map(|i| {
                    let _scope = Scope::enter(playback);
                    let _scheduling = scheduling.enter();
                    let _memo = memo.enter();
                    f(i)
                })
                .collect()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_does_not_wait_for_busy_background_workers() {
        use std::sync::{mpsc::channel, Arc, Barrier};
        use std::time::Duration;
        let Some(pool) = pool() else { return };
        let count = pool.current_num_threads();
        let (started, ready) = channel();
        let release = Arc::new(Barrier::new(count + 1));
        let background_release = release.clone();
        let background = std::thread::spawn(move || {
            map(count, |_| {
                started.send(()).unwrap();
                background_release.wait();
            });
        });
        for _ in 0..count {
            ready.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        let (finished, result) = channel();
        let playback = std::thread::spawn(move || {
            let values = with_playback_workers(|| map(3, |i| map(3, |j| (i, j))));
            finished.send(values).unwrap();
        });
        let timely = result.recv_timeout(Duration::from_secs(2));
        // Always release and join before asserting, including on the old
        // shared-pool implementation: a failed regression must not hang tests.
        release.wait();
        background.join().unwrap();
        playback.join().unwrap();
        assert_eq!(
            timely.expect("playback was queued behind background planning"),
            (0..3)
                .map(|i| (0..3).map(|j| (i, j)).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn scoped_workers_keep_background_qos() {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
            fn qos_class_self() -> u32;
        }
        // Use a dedicated parent so the test runner's QoS is unchanged.
        std::thread::spawn(|| {
            // SAFETY: valid Darwin utility class and relative priority.
            assert_eq!(unsafe { pthread_set_qos_class_self_np(0x11, 0) }, 0);
            let classes = map(40, |_| {
                // SAFETY: parameter-free Darwin thread query.
                unsafe { qos_class_self() }
            });
            assert_eq!(classes, vec![0x11; 40]);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn uneven_jobs_keep_their_input_positions() {
        for count in [0, 1, 3, 100] {
            assert_eq!(
                map(count, |i| {
                    if i % 3 == 0 {
                        std::thread::yield_now();
                    }
                    i * i
                }),
                (0..count).map(|i| i * i).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn nested_forecasts_share_the_bounded_pool_and_keep_order() {
        let Some(pool) = pool() else { return };
        let workers = pool.current_num_threads();
        let jobs = map(workers + 1, |outer| {
            map(workers + 1, |inner| {
                assert_eq!(rayon::current_num_threads(), workers);
                assert!(rayon::current_thread_index().unwrap() < workers);
                (outer, inner)
            })
        });
        assert_eq!(
            jobs,
            (0..=workers)
                .map(|outer| (0..=workers)
                    .map(|inner| (outer, inner))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>()
        );
    }
}
