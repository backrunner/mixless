//! Opt-in CPU draw timings, including layout/paint, for local UI profiling.
//! Enable with MIXLESS_PROFILE_UI=1. This does not measure GPU presentation.
use std::{cell::RefCell, sync::OnceLock, time::Instant};

pub fn begin() -> Option<Instant> {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    ENABLED
        .get_or_init(|| std::env::var_os("MIXLESS_PROFILE_UI").is_some())
        .then(Instant::now)
}

#[derive(Default)]
struct Samples {
    cpu: Vec<f64>,
    intervals: Vec<f64>,
    last: Option<Instant>,
}

pub fn finish(start: Option<Instant>, active_decks: usize) {
    let Some(start) = start else { return };
    thread_local! { static SAMPLES: RefCell<[Samples; 3]> = Default::default(); }
    SAMPLES.with(|samples| {
        let mut samples = samples.borrow_mut();
        let sample = &mut samples[active_decks.min(2)];
        let now = Instant::now();
        sample
            .cpu
            .push(now.duration_since(start).as_secs_f64() * 1000.);
        if let Some(last) = sample.last.replace(now) {
            let elapsed = now.duration_since(last).as_secs_f64() * 1000.;
            if elapsed < 1000. {
                sample.intervals.push(elapsed);
            }
        }
        if sample.cpu.len() >= 240 {
            sample.cpu.sort_by(f64::total_cmp);
            sample.intervals.sort_by(f64::total_cmp);
            tracing::info!(
                active_decks,
                cpu_p50_ms = sample.cpu[120],
                cpu_p99_ms = sample.cpu[237],
                interval_p99_ms = sample
                    .intervals
                    .get(sample.intervals.len() * 99 / 100)
                    .copied()
                    .unwrap_or(0.),
                "UI draw profile (240 frames; CPU only)"
            );
            sample.cpu.clear();
            sample.intervals.clear();
        }
    });
}
