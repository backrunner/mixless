//! AutoMix session intent, worker lifecycle and UI progress.
use super::*;

/// Follow an entrance once; later manual selection remains under user control.
pub(super) fn newly_playing_deck(
    before: &EngineSnapshot,
    after: &EngineSnapshot,
) -> Option<DeckId> {
    [DeckId::A, DeckId::B].into_iter().find(|deck| {
        let (old, new) = (before.deck(*deck), after.deck(*deck));
        new.playing && new.track_id.is_some() && (!old.playing || new.track_id != old.track_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_follows_new_entrance_only_after_playback_starts() {
        let mut before = EngineSnapshot::default();
        before.decks[0].playing = true;
        before.decks[0].track_id = Some(TrackId(1));
        let mut after = before.clone();
        after.decks[1].track_id = Some(TrackId(2));
        assert_eq!(newly_playing_deck(&before, &after), None);
        after.decks[1].playing = true;
        assert_eq!(newly_playing_deck(&before, &after), Some(DeckId::B));
        assert_eq!(newly_playing_deck(&after, &after), None);
    }
}

impl UiState {
    pub(super) fn mix_playlist(&self) -> Option<i64> {
        if self.automix_active {
            self.automix_playlist
        } else {
            self.playlist_sel
        }
    }

    /// The rows the library shows. While AUTO plays this playlist in BEST
    /// order the list follows that order; the playlist itself is untouched
    /// and its own order returns as soon as AUTO stops.
    pub fn shown_tracks(&self) -> Arc<Vec<Track>> {
        if !self.automix_active || self.automix_playlist != self.playlist_sel {
            return self.tracks.clone();
        }
        let ids: Vec<_> = self.tracks.iter().map(|track| track.id).collect();
        let Some(order) = crate::automix::arranged(&self.core, &ids).filter(|o| *o != ids) else {
            return self.tracks.clone();
        };
        let mut cache = self.best_rows.borrow_mut();
        if let Some((source, cached, rows)) = cache.as_ref()
            && Arc::ptr_eq(source, &self.tracks)
            && *cached == order
        {
            return rows.clone();
        }
        let by_id: std::collections::HashMap<_, _> =
            self.tracks.iter().map(|track| (track.id, track)).collect();
        let rows: Arc<Vec<Track>> = Arc::new(
            order
                .iter()
                .filter_map(|id| by_id.get(id).map(|track| (*track).clone()))
                .collect(),
        );
        *cache = Some((self.tracks.clone(), order, rows.clone()));
        rows
    }

    pub(super) fn prepare_mix_playlist(&mut self, tracks: Vec<TrackId>) {
        if self.automix_active {
            self.automix_tracks = tracks.clone();
        }
        crate::automix::prepare_playlist(&self.core, tracks);
    }

    /// ORDER, SHUFFLE and BEST in turn. BEST plays the playlist in the order
    /// that mixes most smoothly, computed in the background.
    pub fn cycle_automix_order(&mut self) {
        let shuffle = self.automix_shuffle.load(Ordering::Acquire);
        let best = self.core.mix_preparation.best();
        let (shuffle, best) = match (shuffle, best) {
            (false, false) => (true, false),
            (true, _) => (false, true),
            (false, true) => (false, false),
        };
        self.automix_shuffle.store(shuffle, Ordering::Release);
        let tracks = if self.automix_active {
            self.automix_tracks.clone()
        } else {
            self.tracks.iter().map(|track| track.id).collect()
        };
        crate::automix::set_best(&self.core, best, tracks);
    }

    pub fn toggle_automix(&mut self) {
        self.sync_request = None;
        if self.automix_active {
            self.stop_automix();
            return;
        }
        let tracks: Vec<_> = self.tracks.iter().map(|track| track.id).collect();
        if tracks.is_empty() {
            self.error = "Import a track to start Automix".into();
            return;
        }
        if let Some(error) = crate::analysis::preparation_error(&self.core, &tracks) {
            self.error = format!("AutoMix cannot prepare tracks: {error}").into();
            return;
        }
        for epoch in self.deck_load_epoch.iter() {
            epoch.fetch_add(1, Ordering::AcqRel);
        }
        self.deck_load_rx = [None, None];
        self.deck_loading = [None, None];
        for loading in &self.core.controller_loading {
            loading.store(false, Ordering::Release);
        }
        self.grid_rx = [None, None];
        self.automix_playlist = self.playlist_sel;
        self.automix_tracks = tracks.clone();
        // The smoothest order is the default; an explicit ORDER, BEST or
        // SHUFFLE choice is kept instead.
        if !self.automix_shuffle.load(Ordering::Acquire) {
            crate::automix::default_to_best(&self.core, tracks.clone());
        }
        // BEST needs a planned pair for every neighbouring pair, so the set
        // is analysed and ordered first; AUTO shows that it is working
        // rather than starting on the wrong order.
        if crate::analysis::needs_work(&self.core, &tracks)
            || !crate::automix::order_ready(&self.core, &tracks)
        {
            self.automix_active = true;
            self.automix_waiting = true;
            self.update_preparation_progress();
            return;
        }
        self.start_automix_worker(self.automix_order(tracks));
    }

    /// The set in the order AUTO plays it.
    fn automix_order(&self, tracks: Vec<TrackId>) -> Vec<TrackId> {
        crate::automix::arranged(&self.core, &tracks).unwrap_or(tracks)
    }

    fn start_automix_worker(&mut self, tracks: Vec<TrackId>) {
        self.automix_retry = None;
        self.automix_loading = true;
        self.automix_preparation_progress = None;
        let generation = self.automix_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.automix_active = true;
        self.automix_status = "Loading next track".into();
        self.error = "".into();
        let (tx, rx) = channel();
        self.automix_rx = Some(rx);
        let core = self.core.clone();
        let epoch = self.automix_epoch.clone();
        let shuffle = self.automix_shuffle.clone();
        std::thread::spawn(move || {
            crate::automix::run(core, tracks, shuffle, epoch, generation, tx)
        });
    }

    pub fn stop_automix(&mut self) {
        let core = self.core.clone();
        let _commit = core.automix_commit.lock().expect("automix commit");
        self.automix_epoch.fetch_add(1, Ordering::AcqRel);
        self.automix_active = false;
        self.automix_waiting = false;
        self.automix_loading = false;
        self.automix_preparation_progress = None;
        self.automix_retry = None;
        self.automix_status.clear();
        self.automix_plan = None;
        self.automix_rx = None;
        self.dispatch(Command::StopAutomix);
        drop(_commit);
        self.automix_playlist = None;
        self.automix_tracks.clear();
        self.prepare_mix_playlist(self.tracks.iter().map(|track| track.id).collect());
    }

    pub(super) fn poll_automix(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = self.automix_rx.take() {
            let mut finished = false;
            while let Ok(message) = rx.try_recv() {
                changed = true;
                match message {
                    crate::automix::AutomixMsg::Status(status) => self.automix_status = status,
                    crate::automix::AutomixMsg::Preparing(title) => {
                        self.automix_plan = None;
                        self.automix_loading = true;
                        self.automix_preparation_progress = None;
                        self.automix_status = format!("Loading {title}");
                    }
                    crate::automix::AutomixMsg::Plan(deck, plan) => {
                        self.automix_plan = Some((deck, plan));
                        self.automix_loading = false;
                        self.automix_preparation_progress = None;
                    }
                    crate::automix::AutomixMsg::Done(result) => {
                        finished = true;
                        self.automix_loading = false;
                        self.automix_preparation_progress = None;
                        self.automix_plan = None;
                        self.automix_status = match result {
                            Ok(()) => "Waiting for playback".into(),
                            Err(error) => {
                                tracing::debug!("Automix waiting: {error}");
                                "Waiting for a playable transition".into()
                            }
                        };
                        self.automix_retry =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                    }
                }
            }
            if !finished {
                self.automix_rx = Some(rx);
            }
        }
        if self.automix_waiting {
            if let Some(error) =
                crate::analysis::preparation_error(&self.core, &self.automix_tracks)
            {
                self.stop_automix();
                self.error = format!("AutoMix cannot prepare tracks: {error}").into();
                changed = true;
            } else if crate::analysis::needs_work(&self.core, &self.automix_tracks) {
                changed |= self.update_preparation_progress();
            } else if !crate::automix::order_ready(&self.core, &self.automix_tracks) {
                changed |= self.update_preparation_progress();
            } else {
                self.automix_waiting = false;
                self.start_automix_worker(self.automix_order(self.automix_tracks.clone()));
                changed = true;
            }
        }
        if self.automix_active
            && self
                .automix_retry
                .is_some_and(|time| std::time::Instant::now() >= time)
            && self.deck_load_rx.iter().all(Option::is_none)
            && !self.automix_tracks.is_empty()
        {
            self.start_automix_worker(self.automix_order(self.automix_tracks.clone()));
            changed = true;
        }
        changed
    }

    fn update_preparation_progress(&mut self) -> bool {
        let (ready, total) = crate::analysis::work_progress(&self.core, &self.automix_tracks);
        let (status, progress) = if ready < total {
            (
                format!("Analyzing tracks {ready}/{total}"),
                Some(ready as f32 / total as f32),
            )
        } else {
            (
                "Finding the best order".into(),
                self.core
                    .mix_preparation
                    .order_progress(&self.automix_tracks),
            )
        };
        let changed =
            self.automix_status != status || self.automix_preparation_progress != progress;
        self.automix_status = status;
        self.automix_preparation_progress = progress;
        self.automix_loading = true;
        changed
    }
}
