//! Background ordering and progress owned by one playlist request.
use super::order_cache::{Arranged, Stamp};
use super::*;
use std::sync::atomic::AtomicUsize;

pub(super) struct OrderingJob {
    playlist: Vec<TrackId>,
    done: AtomicUsize,
    total: AtomicUsize,
}

impl OrderingJob {
    fn new(playlist: Vec<TrackId>) -> Self {
        Self {
            playlist,
            done: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
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
mod tests {
    use super::*;

    #[test]
    fn completed_orders_are_reused_after_switching_playlists_and_clearing_memory() {
        let (_dir, core, tracks) = super::super::super::tests::fixture();
        let prepared: Vec<_> = tracks
            .iter()
            .map(|id| crate::analysis::prepare(&core, *id).unwrap())
            .collect();
        let options = mixless_mixplan::PlannerOptions {
            harmonic_key_shift: true,
            live_moves: core.settings.get().live_moves,
            ..Default::default()
        };
        let key = super::super::order_cache::key(&prepared, &options).unwrap();
        // A recognizable persisted permutation proves the worker takes the hit
        // instead of running the optimizer and producing its own answer.
        core.library.save_best_order(&key, &[2, 0, 1]).unwrap();
        core.mix_preparation.best.store(true, Ordering::Release);
        let expected = vec![tracks[2], tracks[0], tracks[1]];
        let wait = || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while core.mix_preparation.arranging.lock().unwrap().is_some() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "ordering did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        for input in [
            tracks.clone(),
            vec![tracks[1], tracks[2], tracks[0]],
            tracks.clone(),
        ] {
            *core.mix_preparation.arranged.lock().unwrap() = None;
            arrange(&core, input.clone());
            wait();
            assert!(order_ready(&core, &input));
            if input == tracks {
                assert_eq!(arranged(&core, &tracks), Some(expected.clone()));
            }
        }
        core.analysis
            .content_revision
            .fetch_add(1, Ordering::Release);
        let cached = core.mix_preparation.arranged.lock().unwrap();
        assert!(cached.as_ref().unwrap().stamp != Some(Stamp::current(&core)));
    }

    #[test]
    fn progress_is_monotonic_and_belongs_to_its_playlist_job() {
        let preparation = Preparation::default();
        let playlist = vec![TrackId(1), TrackId(2), TrackId(3)];
        let job = Arc::new(OrderingJob::new(playlist.clone()));
        *preparation.arranging.lock().unwrap() = Some(job.clone());
        assert_eq!(preparation.order_progress(&playlist), None);
        job.report(40, 100);
        job.report(20, 100);
        assert_eq!(preparation.order_progress(&playlist), Some(0.4));
        assert_eq!(preparation.order_progress(&[TrackId(4)]), None);
        let replacement = Arc::new(OrderingJob::new(playlist.clone()));
        *preparation.arranging.lock().unwrap() = Some(replacement.clone());
        job.report(100, 100);
        assert_eq!(preparation.order_progress(&playlist), None);
        replacement.report(10, 100);
        assert_eq!(preparation.order_progress(&playlist), Some(0.1));
        *preparation.arranging.lock().unwrap() = None;
        assert_eq!(preparation.order_progress(&playlist), None);
    }
}

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
            || !core.mix_preparation.best()
            || playlist.len() < 3
        {
            *arranging = None;
        }
    }
    if !core.mix_preparation.best() || playlist.len() < 3 {
        return playlist;
    }
    // An order measured while tracks were still being analysed is used
    // until the set is ready, then measured again.
    let mut shown = playlist.clone();
    if let Some(cached) = core
        .mix_preparation
        .arranged
        .lock()
        .expect("arranged")
        .as_ref()
        && cached.input == playlist
    {
        if cached.stamp == Some(Stamp::current(core))
            || crate::analysis::needs_work(core, &playlist)
        {
            return cached.order.clone();
        }
        shown = cached.order.clone();
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
        let indices = cached.unwrap_or_else(|| {
            mixless_mixplan::order::best_cycle_with_progress(&analyses, &options, |done, total| {
                job.report(done, total)
            })
            .into_iter()
            .map(|k| known[k])
            .chain((0..input.len()).filter(|&i| prepared[i].is_none()))
            .collect()
        });
        let complete = stable();
        if complete && let Some(key) = cache_key.as_deref() {
            if !hit && let Err(error) = core.library.save_best_order(key, &indices) {
                tracing::warn!(%error, "Best order cache write failed");
            }
            tracing::debug!(hit, tracks = input.len(), "Best order cache");
        }
        let arranged: Vec<_> = indices.iter().map(|&i| input[i]).collect();
        let mut arranging = core.mix_preparation.arranging.lock().expect("arranging");
        if !arranging
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &job))
        {
            return;
        }
        *core.mix_preparation.arranged.lock().expect("arranged") = Some(Arranged {
            input: input.clone(),
            order: arranged.clone(),
            stamp: complete.then_some(stamp),
        });
        *arranging = None;
        drop(arranging);
        // Hold selection through publication so a late worker cannot switch
        // back to the playlist the user just left.
        let selected = core
            .mix_preparation
            .selected
            .lock()
            .expect("selected playlist");
        if core.mix_preparation.best() && *selected == input {
            prepare(&core, arranged, false, true);
        }
    });
    shown
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
        .filter(|cached| cached.input == playlist)
        .map(|cached| cached.order.clone())
}

/// Whether AUTO may start playing: BEST is off, or its order for this
/// playlist has been measured on the finished analyses. Starts measuring
/// when nothing is doing so yet.
pub fn order_ready(core: &Arc<AppCore>, playlist: &[TrackId]) -> bool {
    if !core.mix_preparation.best() || playlist.len() < 3 {
        return true;
    }
    let ready = matches!(
        core.mix_preparation.arranged.lock().expect("arranged").as_ref(),
        Some(cached) if cached.input == playlist && cached.stamp == Some(Stamp::current(core))
    );
    let measuring = core
        .mix_preparation
        .arranging
        .lock()
        .expect("arranging")
        .as_ref()
        .is_some_and(|job| job.playlist == playlist);
    if !ready && !measuring {
        prepare_playlist(core, playlist.to_vec());
    }
    ready
}
