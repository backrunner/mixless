//! All imports and individual retries share one platform budget.
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

struct State {
    active: usize,
    next: Instant,
    cooldown: Instant,
}
struct Budget {
    state: Mutex<State>,
    wake: Condvar,
    spacing: Duration,
}
fn budget() -> &'static Budget {
    static BUDGET: OnceLock<Budget> = OnceLock::new();
    BUDGET.get_or_init(|| Budget::new(Duration::from_secs(1)))
}
impl Budget {
    fn new(spacing: Duration) -> Self {
        Self {
            state: Mutex::new(State {
                active: 0,
                next: Instant::now(),
                cooldown: Instant::now(),
            }),
            wake: Condvar::new(),
            spacing,
        }
    }
}
pub struct Permit<'a>(&'a Budget);
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let budget = self.0;
        budget.state.lock().unwrap().active -= 1;
        budget.wake.notify_all();
    }
}

pub fn acquire(deadline: Instant) -> Result<Permit<'static>, String> {
    acquire_from(budget(), deadline)
}

fn acquire_from(budget: &Budget, deadline: Instant) -> Result<Permit<'_>, String> {
    let mut state = budget.state.lock().unwrap();
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err("Waiting for the download rate limit; retry later".into());
        }
        if state.cooldown > now {
            return Err(format!(
                "YouTube rate limited; retry after {} seconds",
                (state.cooldown - now).as_secs() + 1
            ));
        }
        if state.active < 2 && state.next <= now {
            state.active += 1;
            state.next = now + budget.spacing;
            return Ok(Permit(budget));
        }
        let wait = if state.active >= 2 {
            Duration::from_millis(250)
        } else {
            state.next - now
        };
        state = budget
            .wake
            .wait_timeout(state, wait.min(deadline - now))
            .unwrap()
            .0;
    }
}

pub fn rate_limited(seconds: u64) {
    defer(budget(), seconds);
}

fn defer(budget: &Budget, seconds: u64) {
    let mut state = budget.state.lock().unwrap();
    state.cooldown = state
        .cooldown
        .max(Instant::now() + Duration::from_secs(seconds.clamp(1, 86400)));
    budget.wake.notify_all();
}

pub fn is_rate_limit(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("429")
        || message.contains("too many requests")
        || message.contains("rate limit")
        || message.contains("try again later")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrency_spacing_deadlines_and_shared_cooldown() {
        let budget = Budget::new(Duration::from_millis(25));
        let first = acquire_from(&budget, Instant::now() + Duration::from_secs(1)).unwrap();
        let started = Instant::now();
        let second = acquire_from(&budget, Instant::now() + Duration::from_secs(1)).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(20));
        assert!(acquire_from(&budget, Instant::now() + Duration::from_millis(10)).is_err());
        drop(first);
        let third = acquire_from(&budget, Instant::now() + Duration::from_secs(1)).unwrap();
        defer(&budget, 60);
        let cooldown = budget.state.lock().unwrap().cooldown;
        defer(&budget, 1);
        assert_eq!(budget.state.lock().unwrap().cooldown, cooldown);
        drop(second);
        drop(third);
        let error = acquire_from(&budget, Instant::now() + Duration::from_secs(1))
            .err()
            .unwrap();
        assert!(error.contains("rate limited"));
        assert_eq!(budget.state.lock().unwrap().active, 0);
    }
}
