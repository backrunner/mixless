//! Opt-in rejection counters for offline audits. Without the `trace` feature
//! every call compiles to a bare `None`.

/// Reject the current candidate, recording the source line under `trace`.
macro_rules! miss {
    () => {
        crate::trace::record(concat!(file!(), ":", line!()))
    };
}

#[cfg(feature = "trace")]
thread_local! {
    static COUNTS: std::cell::RefCell<std::collections::BTreeMap<&'static str, u32>> =
        Default::default();
}

#[inline(always)]
pub(crate) fn record<T>(_site: &'static str) -> Option<T> {
    #[cfg(feature = "trace")]
    COUNTS.with(|c| *c.borrow_mut().entry(_site).or_default() += 1);
    None
}

/// Drain this thread's counts, most frequent first.
#[cfg(feature = "trace")]
pub fn take() -> Vec<(&'static str, u32)> {
    let mut counts: Vec<_> =
        COUNTS.with(|c| std::mem::take(&mut *c.borrow_mut()).into_iter().collect());
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts
}
