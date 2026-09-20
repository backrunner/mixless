use std::sync::{Condvar, Mutex, OnceLock};

pub fn workers() -> usize {
    mixless_protocol::BackgroundWorkers::detected().analysis
}

struct Slots {
    available: Mutex<usize>,
    wake: Condvar,
}

static SLOTS: OnceLock<Slots> = OnceLock::new();

pub(super) struct Permit {
    slots: &'static Slots,
    _cpu: mixless_protocol::BackgroundCpuPermit<'static>,
}

pub(super) fn acquire() -> Permit {
    let slots = SLOTS.get_or_init(|| Slots {
        available: Mutex::new(workers()),
        wake: Condvar::new(),
    });
    let mut available = slots.available.lock().unwrap_or_else(|p| p.into_inner());
    while *available == 0 {
        available = slots
            .wake
            .wait(available)
            .unwrap_or_else(|p| p.into_inner());
    }
    *available -= 1;
    drop(available);
    Permit {
        slots,
        _cpu: mixless_protocol::BackgroundWorkers::acquire_cpu(1, || true)
            .expect("analysis CPU reservation is not cancellable"),
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        *self
            .slots
            .available
            .lock()
            .unwrap_or_else(|p| p.into_inner()) += 1;
        self.slots.wake.notify_one();
    }
}
