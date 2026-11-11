//! One bounded work-stealing pool for pair, entrance and lookahead work.
//! Nested requests share workers rather than multiplying OS threads.
use rayon::prelude::*;
use std::sync::OnceLock;

fn pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(std::thread::available_parallelism().map_or(4, |n| n.get()))
            .thread_name(|i| format!("mixless-plan-{i}"))
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
    let f = &f;
    crate::memo::with_workers(|memo| {
        pool.install(|| {
            (0..count)
                .into_par_iter()
                .map(|i| {
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
