//! Background ordering and progress owned by one playlist request.
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
    if !core.mix_preparation.best() || playlist.len() < 3 {
        return playlist;
    }
    // An order measured while tracks were still being analysed is used
    // until the set is ready, then measured again.
    let mut shown = playlist.clone();
    if let Some((input, arranged, complete)) = core
        .mix_preparation
        .arranged
        .lock()
        .expect("arranged")
        .as_ref()
        && *input == playlist
    {
        if *complete || crate::analysis::needs_work(core, &playlist) {
            return arranged.clone();
        }
        shown = arranged.clone();
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
        let prepared: Vec<_> = input
            .iter()
            .map(|id| crate::analysis::prepare(&core, *id).ok())
            .collect();
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
        let mut arranged: Vec<TrackId> =
            mixless_mixplan::order::best_cycle_with_progress(&analyses, &options, |done, total| {
                job.report(done, total)
            })
            .into_iter()
            .map(|k| input[known[k]])
            .collect();
        // Tracks still being analyzed keep their playlist position order at the end.
        arranged.extend(
            (0..input.len())
                .filter(|i| prepared[*i].is_none())
                .map(|i| input[i]),
        );
        let complete = known.len() == input.len() && !crate::analysis::needs_work(&core, &input);
        let mut arranging = core.mix_preparation.arranging.lock().expect("arranging");
        if !arranging
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &job))
        {
            return;
        }
        *core.mix_preparation.arranged.lock().expect("arranged") =
            Some((input.clone(), arranged, complete));
        *arranging = None;
        drop(arranging);
        let current = core.mix_preparation.tracks();
        if core.mix_preparation.best() && current == input {
            prepare_playlist(&core, input);
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
        .filter(|(input, _, _)| input == playlist)
        .map(|(_, arranged, _)| arranged.clone())
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
        Some((input, _, true)) if input == playlist
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
