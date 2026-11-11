//! Order selection and cache-only activation for one playlist source.
use super::*;

impl Preparation {
    pub fn best(&self) -> bool {
        self.best.load(Ordering::Acquire)
    }
    /// The current source has an explicit order choice.
    pub fn order_chosen(&self) -> bool {
        self.chosen.load(Ordering::Acquire)
    }
}

/// Switch the set between playlist order and best mixing order. `playlist`
/// is the order the user sees; the arranged order replaces it for AUTO only.
#[cfg(test)]
pub fn set_best(core: &Arc<AppCore>, on: bool, playlist: Vec<TrackId>) {
    set_order_mode(
        core,
        if on {
            OrderMode::Best
        } else {
            OrderMode::Order
        },
        playlist,
    );
}

pub fn set_order_mode(core: &Arc<AppCore>, mode: OrderMode, playlist: Vec<TrackId>) {
    let on = mode == OrderMode::Best;
    // Serialize intent and queue publication with a finishing Best worker.
    let mut selected = core
        .mix_preparation
        .selected
        .lock()
        .expect("selected playlist");
    {
        let job = core.mix_preparation.arranging.lock().expect("arranging");
        core.mix_preparation.best.store(on, Ordering::Release);
        core.mix_preparation.chosen.store(true, Ordering::Release);
        core.mix_preparation
            .defaulting
            .store(false, Ordering::Release);
        if !on && job.is_some() {
            // Finish the useful cache in the background without holding the
            // user in Best or starting another search on the next switch.
            core.mix_preparation.warming.store(true, Ordering::Release);
        }
    }
    *selected = playlist.clone();
    core.mix_preparation.request_mode(mode);
    // ORDER/SHUFFLE never need to read or wait for the Best result cache.
    let tracks = if on {
        let tracks = arrange(core, playlist.clone());
        if super::ordering::arranged(core, &playlist).is_none() && playlist.len() >= 3 {
            // Keep the applied ORDER/SHUFFLE queue while Best is being computed.
            if core.mix_preparation.tracks().is_empty() {
                prepare(core, playlist, false, true);
            }
            return;
        }
        core.mix_preparation.best_ready();
        tracks
    } else {
        playlist
    };
    prepare(core, tracks, true, true);
}

/// Begin a different source's order selection. `None` requests its default;
/// `Some` restores its explicit Best/Order choice (Shuffle is owned by UI).
/// Invalidate the old request even when the two playlists contain the same IDs.
pub fn reset_order_for_playlist(core: &Arc<AppCore>, best: Option<bool>) {
    let mut selected = core
        .mix_preparation
        .selected
        .lock()
        .expect("selected playlist");
    *selected = Vec::new();
    {
        let mut sequence = core.mix_preparation.sequence.lock().expect("set plan");
        sequence.queue.reset(OrderMode::Order);
        sequence.anchor_entries = None;
    }
    {
        let mut job = core.mix_preparation.arranging.lock().expect("arranging");
        *job = None;
        core.mix_preparation.best.store(false, Ordering::Release);
        core.mix_preparation
            .chosen
            .store(best.is_some(), Ordering::Release);
        core.mix_preparation
            .defaulting
            .store(false, Ordering::Release);
    }
    // Keep the engine's committed handoff; new source rows arrive asynchronously.
    prepare(core, Vec::new(), false, true);
}

/// Prefer a ready Best cache. On a miss this session keeps playlist order;
/// warming the cache never switches a running session into Best.
pub fn default_to_best(core: &Arc<AppCore>, playlist: Vec<TrackId>) {
    if core.mix_preparation.order_chosen() || playlist.len() < 3 {
        return;
    }
    prefer_cached_best(core, playlist);
}

/// Restore a source's Best preference only after a valid cache hit. A miss
/// keeps this pass in ORDER even if this source explicitly chose Best before.
pub fn prefer_cached_best(core: &Arc<AppCore>, playlist: Vec<TrackId>) {
    core.mix_preparation.request_mode(OrderMode::Order);
    {
        let _job = core.mix_preparation.arranging.lock().expect("arranging");
        core.mix_preparation.best.store(false, Ordering::Release);
        core.mix_preparation.warming.store(true, Ordering::Release);
        core.mix_preparation
            .defaulting
            .store(playlist.len() >= 3, Ordering::Release);
    }
    prepare_playlist(core, playlist);
}
