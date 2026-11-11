//! Admit whole recordings before allocating PCM or loading model sessions.
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pressure {
    Normal,
    Warning,
    Critical,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
struct Snapshot {
    total: Option<u64>,
    available: Option<u64>,
    pressure: Pressure,
}

impl Snapshot {
    fn headroom(self) -> u64 {
        (self.total.unwrap_or(8 * GIB) / 8).clamp(GIB / 2, 8 * GIB)
    }
    fn bytes(self, seconds: f32) -> u64 {
        // Model/activation reserve plus source PCM (up to 192 kHz), resampled
        // mixture, four lanes and encoding/verification scratch. Reserve future
        // allocations as well as the memory already visible in VM statistics.
        let fixed = (self.total.unwrap_or(8 * GIB) / 2).clamp(GIB, 4 * GIB);
        fixed.saturating_add(
            (f64::from(seconds.max(0.)) * (192000. * 8. + 44100. * 8. * 7.)).ceil() as u64,
        )
    }
}

#[derive(Default)]
struct State {
    jobs: usize,
    bytes: u64,
}

struct Budget {
    capacity: usize,
    state: Mutex<State>,
    changed: Condvar,
}

pub(super) struct Permit<'a> {
    budget: &'a Budget,
    bytes: u64,
}

impl Budget {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        }
    }

    fn admits(&self, state: &State, sample: Snapshot, bytes: u64) -> bool {
        let capacity = match sample.pressure {
            Pressure::Critical => 0,
            Pressure::Normal if sample.total.is_some() && sample.available.is_some() => {
                self.capacity
            }
            _ => 1,
        };
        state.jobs < capacity
            && sample.available.map_or(state.jobs == 0, |available| {
                state.bytes.saturating_add(bytes) <= available.saturating_sub(sample.headroom())
            })
    }

    fn acquire(
        &self,
        seconds: f32,
        sample: impl Fn() -> Snapshot,
        active: &impl Fn() -> bool,
        mut waiting: impl FnMut(),
    ) -> crate::Result<Permit<'_>> {
        loop {
            if !active() {
                return Err(crate::Error::Cancelled);
            }
            let snapshot = sample();
            let bytes = snapshot.bytes(seconds);
            if snapshot
                .total
                .is_some_and(|total| bytes > total.saturating_sub(snapshot.headroom()))
            {
                return Err(crate::Error::Model(
                    "This track needs more memory for stem separation than this system can reserve"
                        .into(),
                ));
            }
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if self.admits(&state, snapshot, bytes) {
                state.jobs += 1;
                state.bytes += bytes;
                tracing::debug!(jobs = state.jobs, capacity = self.capacity, reserved_bytes = state.bytes, available_bytes = ?snapshot.available, "Stem resource admission");
                return Ok(Permit {
                    budget: self,
                    bytes,
                });
            }
            drop(state);
            // Session destruction must not run under the admission mutex.
            waiting();
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            drop(
                self.changed
                    .wait_timeout(state, Duration::from_millis(250))
                    .unwrap_or_else(|p| p.into_inner()),
            );
        }
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut state = self.budget.state.lock().unwrap_or_else(|p| p.into_inner());
        state.jobs -= 1;
        state.bytes -= self.bytes;
        self.budget.changed.notify_all();
    }
}

pub(super) fn acquire(
    seconds: f32,
    active: &impl Fn() -> bool,
    waiting: impl FnMut(),
) -> crate::Result<Permit<'static>> {
    static BUDGET: OnceLock<Budget> = OnceLock::new();
    BUDGET
        .get_or_init(|| Budget::new(mixless_protocol::BackgroundWorkers::detected().stems))
        .acquire(seconds, snapshot, active, waiting)
}

fn snapshot() -> Snapshot {
    let total = mixless_protocol::BackgroundWorkers::memory_bytes();
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        static HOST: OnceLock<libc::mach_port_t> = OnceLock::new();
        unsafe extern "C" {
            fn mach_host_self() -> libc::mach_port_t;
        }
        // Retain one process-wide host port rather than leaking a send right
        // on each sample. A zeroed statistics structure contains only integers.
        let host = *HOST.get_or_init(|| unsafe { mach_host_self() });
        let mut stats: libc::vm_statistics64_data_t = unsafe { std::mem::zeroed() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        let status = unsafe {
            libc::host_statistics64(
                host,
                libc::HOST_VM_INFO64,
                (&mut stats as *mut libc::vm_statistics64_data_t).cast(),
                &mut count,
            )
        };
        // Speculative pages are included in free_count, and purgeable pages
        // overlap inactive memory: neither is added again.
        let available = (status == libc::KERN_SUCCESS && count >= 3).then(|| {
            (u64::from(stats.free_count) + u64::from(stats.inactive_count))
                .saturating_mul(unsafe { libc::vm_page_size } as u64)
        });
        let mut pressure = 0u32;
        let mut size = std::mem::size_of_val(&pressure);
        let status = unsafe {
            libc::sysctlbyname(
                c"kern.memorystatus_vm_pressure_level".as_ptr(),
                (&mut pressure as *mut u32).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        let pressure = match (status, pressure) {
            (0, 1) => Pressure::Normal,
            (0, 2) => Pressure::Warning,
            (0, 4) => Pressure::Critical,
            _ => Pressure::Unknown,
        };
        return Snapshot {
            total,
            available,
            pressure,
        };
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    Snapshot {
        total,
        available: None,
        pressure: Pressure::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn roomy() -> Snapshot {
        Snapshot {
            total: Some(64 * GIB),
            available: Some(48 * GIB),
            pressure: Pressure::Normal,
        }
    }

    #[test]
    fn memory_pressure_and_reserved_allocations_bound_parallel_jobs() {
        let budget = Budget::new(4);
        let sample = roomy();
        let permits: Vec<_> = (0..4)
            .map(|_| {
                budget
                    .acquire(240., roomy, &|| true, || panic!("enough resources"))
                    .unwrap()
            })
            .collect();
        let state = budget.state.lock().unwrap();
        assert!(!budget.admits(&state, sample, sample.bytes(240.)));
        assert!(!budget.admits(
            &state,
            Snapshot {
                pressure: Pressure::Warning,
                ..sample
            },
            1
        ));
        drop(state);
        drop(permits);
        let state = budget.state.lock().unwrap();
        assert_eq!((state.jobs, state.bytes), (0, 0));
        assert!(!budget.admits(
            &state,
            Snapshot {
                pressure: Pressure::Critical,
                ..sample
            },
            1
        ));
        assert!(!budget.admits(
            &state,
            Snapshot {
                available: Some(sample.headroom()),
                ..sample
            },
            1
        ));
        assert!(budget.admits(
            &state,
            Snapshot {
                pressure: Pressure::Warning,
                ..sample
            },
            sample.bytes(240.)
        ));
    }

    #[test]
    fn pressure_recovery_and_cancellation_do_not_leak_reservations() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let budget = Budget::new(4);
        let recovered = AtomicBool::new(false);
        let permit = budget
            .acquire(
                240.,
                || Snapshot {
                    pressure: if recovered.load(Ordering::Relaxed) {
                        Pressure::Normal
                    } else {
                        Pressure::Critical
                    },
                    ..roomy()
                },
                &|| true,
                || {
                    recovered.store(true, Ordering::Relaxed);
                },
            )
            .unwrap();
        drop(permit);
        let active = AtomicBool::new(true);
        assert!(budget
            .acquire(
                240.,
                || Snapshot {
                    pressure: Pressure::Critical,
                    ..roomy()
                },
                &|| active.load(Ordering::Relaxed),
                || active.store(false, Ordering::Relaxed)
            )
            .is_err());
        assert_eq!(budget.state.lock().unwrap().jobs, 0);
        let sample = Snapshot {
            available: None,
            pressure: Pressure::Unknown,
            total: None,
        };
        let first = budget
            .acquire(240., || sample, &|| true, || panic!("single fallback"))
            .unwrap();
        assert!(!budget.admits(&budget.state.lock().unwrap(), sample, sample.bytes(240.)));
        assert!(!budget.admits(
            &budget.state.lock().unwrap(),
            Snapshot {
                total: None,
                ..roomy()
            },
            1
        ));
        drop(first);
    }

    #[test]
    fn an_impossible_recording_fails_instead_of_waiting_forever() {
        let budget = Budget::new(1);
        let sample = Snapshot {
            total: Some(8 * GIB),
            available: Some(8 * GIB),
            pressure: Pressure::Normal,
        };
        assert!(matches!(
            budget.acquire(
                1200.,
                || sample,
                &|| true,
                || panic!("cannot recover enough RAM")
            ),
            Err(crate::Error::Model(_))
        ));
        assert!(matches!(
            budget.acquire(1200., || sample, &|| false, || {}),
            Err(crate::Error::Cancelled)
        ));
        assert_eq!(budget.state.lock().unwrap().jobs, 0);
    }
}
