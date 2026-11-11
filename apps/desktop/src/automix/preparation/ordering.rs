//! Background ordering and progress owned by one playlist request.
use super::order_cache::{Arranged, Stamp};
use super::*;
use std::sync::atomic::AtomicUsize;

pub(super) struct OrderingJob {
    playlist: Vec<TrackId>,
    done: AtomicUsize,
    total: AtomicUsize,
    lookup_done: AtomicBool,
}

struct WorkerGuard {
    core: Arc<AppCore>,
    job: Arc<OrderingJob>,
}
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        let mut arranging = self
            .core
            .mix_preparation
            .arranging
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if arranging
            .as_ref()
            .is_some_and(|job| Arc::ptr_eq(job, &self.job))
        {
            *arranging = None;
            self.core
                .mix_preparation
                .defaulting
                .store(false, Ordering::Release);
            self.core
                .mix_preparation
                .sequence
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .queue
                .error =
                Some("Best order could not be calculated. Select Best order to retry.".into());
            self.core
                .mix_preparation
                .preview_revision
                .fetch_add(1, Ordering::Release);
        }
    }
}

impl OrderingJob {
    pub(super) fn new(playlist: Vec<TrackId>) -> Self {
        Self {
            playlist,
            done: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            lookup_done: AtomicBool::new(false),
        }
    }
    fn report(&self, done: usize, total: usize) {
        self.total.store(total, Ordering::Release);
        // Pair workers can report out of order. Never move progress backwards.
        self.done.fetch_max(done.min(total), Ordering::Release);
    }
    fn fraction(&self) -> Option<f32> {
        let total = self.total.load(Ordering::Acquire);
        (total > 0).then(|| (self.done.load(Ordering::Acquire) as f32 / total as f32).clamp(0., 1.))
    }
}

impl Preparation {
    pub fn order_progress(&self, playlist: &[TrackId]) -> Option<f32> {
        self.arranging
            .lock()
            .expect("arranging")
            .as_ref()
            .filter(|job| job.playlist == playlist)
            .and_then(|job| job.fraction())
    }
}

#[cfg(test)]
mod tests;

/// The best order when it is known for this playlist; otherwise the playlist
/// itself while a worker measures it and republishes the set when done.
/// The order is a loop decided once per playlist: it does not depend on the
/// track on air, so starting or toggling AUTO later never reshuffles it.
pub(super) fn arrange(core: &Arc<AppCore>, playlist: Vec<TrackId>) -> Vec<TrackId> {
    {
        let mut arranging = core.mix_preparation.arranging.lock().expect("arranging");
        if arranging
            .as_ref()
            .is_some_and(|job| job.playlist != playlist)
            || (!core.mix_preparation.best()
                && !core.mix_preparation.warming.load(Ordering::Acquire))
            || playlist.len() < 3
        {
            *arranging = None;
        }
    }
    if (!core.mix_preparation.best() && !core.mix_preparation.warming.load(Ordering::Acquire))
        || playlist.len() < 3
    {
        return playlist;
    }
    // An order measured while tracks were still being analysed is used
    // until the set is ready, then measured again.
    let shown = playlist.clone();
    if let Some(cached) = core
        .mix_preparation
        .arranged
        .lock()
        .expect("arranged")
        .as_ref()
        && cached.input == playlist
    {
        if cached.stamp == Some(Stamp::current(core)) {
            if cached.stamp == Some(Stamp::current(core))
                && core
                    .mix_preparation
                    .defaulting
                    .swap(false, Ordering::AcqRel)
            {
                core.mix_preparation.best.store(true, Ordering::Release);
                core.mix_preparation.best_ready();
            }
            return if core.mix_preparation.best() {
                cached.order.clone()
            } else {
                playlist
            };
        }
    }
    let job = {
        let mut arranging = core.mix_preparation.arranging.lock().expect("arranging");
        if arranging
            .as_ref()
            .is_some_and(|job| job.playlist == playlist)
        {
            return shown;
        }
        let job = Arc::new(OrderingJob::new(playlist.clone()));
        *arranging = Some(job.clone());
        job
    };
    let weak = Arc::downgrade(core);
    let input = playlist.clone();
    std::thread::spawn(move || {
        let Some(core) = weak.upgrade() else { return };
        let _guard = WorkerGuard {
            core: core.clone(),
            job: job.clone(),
        };
        background_priority();
        let mut stamp = Stamp::current(&core);
        let mut prepared: Vec<_> = Vec::new();
        // Warming the analysis cache changes readiness. Re-read a stable
        // snapshot before looking up disk results, including on first launch.
        for _ in 0..3 {
            stamp = Stamp::current(&core);
            prepared = input
                .iter()
                .map(|id| crate::analysis::prepare(&core, *id).ok())
                .collect();
            if stamp == Stamp::current(&core) {
                break;
            }
        }
        let known: Vec<usize> = (0..input.len())
            .filter(|i| prepared[*i].is_some())
            .collect();
        let analyses: Vec<_> = known
            .iter()
            .map(|i| prepared[*i].as_ref().unwrap().analysis.as_ref())
            .collect();
        // Score the transitions the set will really play.
        let options = mixless_mixplan::PlannerOptions {
            harmonic_key_shift: true,
            stem_playback: known
                .iter()
                .all(|i| stem_playback_ready(&core, prepared[*i].as_ref().unwrap())),
            live_moves: core.settings.get().live_moves,
            ..Default::default()
        };
        let stable = || {
            known.len() == input.len()
                && !crate::analysis::needs_work(&core, &input)
                && stamp == Stamp::current(&core)
        };
        // Do not spend CPU optimizing an unfinished snapshot that will change
        // again when analysis completes. Preparation will retry with that data.
        if !stable() {
            let mut arranging = core.mix_preparation.arranging.lock().expect("arranging");
            if arranging
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &job))
            {
                *arranging = None;
                if known.len() != input.len() && !crate::analysis::needs_work(&core, &input) {
                    core.mix_preparation
                        .sequence
                        .lock()
                        .expect("set plan")
                        .queue
                        .error = Some(
                        "Best order needs playable tracks. Select Best order to retry.".into(),
                    );
                    core.mix_preparation
                        .defaulting
                        .store(false, Ordering::Release);
                    core.mix_preparation
                        .preview_revision
                        .fetch_add(1, Ordering::Release);
                }
            }
            return;
        }
        let cache_key = stable()
            .then(|| {
                let tracks: Vec<_> = prepared.iter().flatten().cloned().collect();
                super::order_cache::key(&tracks, &options).ok()
            })
            .flatten();
        let cached = cache_key.as_deref().and_then(|key| {
            core.library
                .load_best_order(key, input.len())
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, "Best order cache read failed");
                    None
                })
        });
        let hit = cached.is_some();
        if !hit {
            // Release automatic startup before doing any expensive search.
            let arranging = core.mix_preparation.arranging.lock().expect("arranging");
            if !arranging
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &job))
            {
                return;
            }
            core.mix_preparation
                .defaulting
                .store(false, Ordering::Release);
            job.lookup_done.store(true, Ordering::Release);
        }
        let pairs = (!hit)
            .then(|| {
                let tracks: Vec<_> = prepared.iter().flatten().cloned().collect();
                super::pair_cache::Pairs::load(&core.library, &tracks, &options)
            })
            .flatten();
        let indices = if let Some(cached) = cached {
            cached
        } else {
            let active = || {
                !core.shutting_down.load(Ordering::Acquire)
                    && stamp == Stamp::current(&core)
                    && core
                        .mix_preparation
                        .arranging
                        .lock()
                        .expect("arranging")
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &job))
            };
            #[cfg(debug_assertions)]
            crate::queue_preview::wait_for_best(active);
            let Some(order) = mixless_mixplan::order::best_cycle_while_cached(
                &analyses,
                &options,
                |done, total| job.report(done, total),
                active,
                pairs
                    .as_ref()
                    .map(|pairs| pairs as &dyn mixless_mixplan::order::PairCostCache),
            ) else {
                let mut arranging = core.mix_preparation.arranging.lock().expect("arranging");
                if arranging
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &job))
                {
                    *arranging = None;
                }
                return;
            };
            order
                .into_iter()
                .map(|k| known[k])
                .chain((0..input.len()).filter(|&i| prepared[i].is_none()))
                .collect()
        };
        let complete = stable();
        if complete && let Some(pairs) = pairs {
            pairs.save(&core.library);
        }
        if complete && let Some(key) = cache_key.as_deref() {
            if !hit && let Err(error) = core.library.save_best_order(key, &indices) {
                tracing::warn!(%error, "Best order cache write failed");
            }
            tracing::debug!(hit, tracks = input.len(), "Best order cache");
        }
        let arranged: Vec<_> = indices.iter().map(|&i| input[i]).collect();
        publish_order(&core, &job, arranged, complete.then_some(stamp), hit);
    });
    shown
}

/// Cache a completed search, applying it only to the still-selected Best mode.
/// Selection stays locked through publication, including the mode check.
pub(super) fn publish_order(
    core: &Arc<AppCore>,
    job: &Arc<OrderingJob>,
    order: Vec<TrackId>,
    stamp: Option<Stamp>,
    hit: bool,
) -> bool {
    let selected = core
        .mix_preparation
        .selected
        .lock()
        .expect("selected playlist");
    let mut arranging = core.mix_preparation.arranging.lock().expect("arranging");
    if !arranging
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, job))
    {
        return false;
    }
    *core.mix_preparation.arranged.lock().expect("arranged") = Some(Arranged {
        input: job.playlist.clone(),
        order: order.clone(),
        stamp,
    });
    let is_selected = *selected == job.playlist;
    if is_selected
        && stamp.is_some()
        && hit
        && core.mix_preparation.defaulting.load(Ordering::Acquire)
    {
        core.mix_preparation.best.store(true, Ordering::Release);
        core.mix_preparation.best_ready();
    }
    job.lookup_done.store(true, Ordering::Release);
    *arranging = None;
    drop(arranging);
    if is_selected && stamp == Some(Stamp::current(core)) && core.mix_preparation.best() {
        core.mix_preparation.best_ready();
        prepare(core, order, true, true);
    }
    if is_selected && stamp.is_some() && hit {
        // Publish the prepared queue before releasing a default cache hit.
        core.mix_preparation
            .defaulting
            .store(false, Ordering::Release);
    }
    true
}

fn background_priority() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
        }
        // SAFETY: QOS_CLASS_UTILITY (Darwin sys/qos.h) and zero relative
        // priority are valid. This affects only this background thread.
        let result = unsafe { pthread_set_qos_class_self_np(0x11, 0) };
        if result != 0 {
            tracing::warn!(result, "Best order background priority unavailable");
        }
    }
}

/// The set in the order AUTO plays it, when that differs from the playlist.
pub fn arranged(core: &AppCore, playlist: &[TrackId]) -> Option<Vec<TrackId>> {
    if !core.mix_preparation.best() {
        return None;
    }
    core.mix_preparation
        .arranged
        .lock()
        .expect("arranged")
        .as_ref()
        .filter(|cached| cached.input == playlist && cached.stamp == Some(Stamp::current(core)))
        .map(|cached| cached.order.clone())
}

/// Startup only waits for a default cache lookup. A cache miss releases AUTO
/// immediately in playlist order while the optimizer continues in the background.
pub fn order_ready(core: &Arc<AppCore>, playlist: &[TrackId]) -> bool {
    if !core.mix_preparation.defaulting.load(Ordering::Acquire) || playlist.len() < 3 {
        return true;
    }
    let lookup_done = core
        .mix_preparation
        .arranging
        .lock()
        .expect("arranging")
        .as_ref()
        .is_some_and(|job| job.playlist == playlist && job.lookup_done.load(Ordering::Acquire));
    if lookup_done {
        core.mix_preparation
            .defaulting
            .store(false, Ordering::Release);
        return true;
    }
    prepare_playlist(core, playlist.to_vec());
    !core.mix_preparation.defaulting.load(Ordering::Acquire)
}
