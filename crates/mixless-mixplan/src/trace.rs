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

#[cfg(feature = "trace")]
thread_local! {
    /// Candidate exit and entry being judged, for `MIXLESS_TRACE_EXIT`.
    pub(crate) static CANDIDATE: std::cell::Cell<(f32, f32, u16, u8)> =
        const { std::cell::Cell::new((0., 0., 0, 0)) };
}

#[inline(always)]
pub(crate) fn record<T>(_site: &'static str) -> Option<T> {
    #[cfg(feature = "trace")]
    {
        COUNTS.with(|c| *c.borrow_mut().entry(_site).or_default() += 1);
        // MIXLESS_TRACE_EXIT=<seconds> prints every rejection of an exit
        // within a bar of that point.
        if let Some(at) = std::env::var("MIXLESS_TRACE_EXIT")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
        {
            let (end, entry, n, mode) = CANDIDATE.with(|c| c.get());
            if (end - at).abs() < 2. {
                eprintln!("exit {end:.1} entry {entry:.1} n={n} mode={mode}: {_site}");
            }
        }
    }
    None
}

#[inline(always)]
pub(crate) fn candidate(_end: f32, _entry: f32, _n: u16, _mode: u8) {
    #[cfg(feature = "trace")]
    CANDIDATE.with(|c| c.set((_end, _entry, _n, _mode)));
}

/// Drain this thread's counts, most frequent first.
#[cfg(feature = "trace")]
pub fn take() -> Vec<(&'static str, u32)> {
    let mut counts: Vec<_> =
        COUNTS.with(|c| std::mem::take(&mut *c.borrow_mut()).into_iter().collect());
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts
}
