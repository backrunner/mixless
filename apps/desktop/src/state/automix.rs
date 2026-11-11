//! AutoMix session intent, worker lifecycle and UI progress.
use super::*;
mod order_selection;
use order_selection::Mode;
pub(super) use order_selection::OrderSelection;
#[cfg(test)]
mod progress_tests;

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

    #[gpui::test]
    fn leaving_best_releases_startup_wait_and_progress_immediately(cx: &mut gpui::TestAppContext) {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        crate::automix::set_best(&core, true, tracks.clone());
        let state = cx.new(|cx| {
            let mut state = UiState::new(core.clone(), cx);
            state.tracks = Arc::new(
                tracks
                    .iter()
                    .map(|id| core.library.get_track(*id).unwrap())
                    .collect(),
            );
            state.automix_tracks = tracks.clone();
            state.automix_active = true;
            state.automix_waiting = true;
            state.automix_loading = true;
            state.automix_status = "Finding the best order".into();
            state.automix_preparation_progress = Some(0.42);
            state
        });
        state.update(cx, |state, _| {
            state.cycle_automix_order();
            assert!(!core.mix_preparation.best());
            assert!(!state.automix_waiting);
            assert!(state.automix_preparation_progress.is_none());
            assert_eq!(state.automix_status, "Loading next track");
            assert_eq!(core.mix_preparation.tracks(), tracks);
            state.stop_automix();
        });
        core.shutting_down.store(true, Ordering::Release);
    }

    #[gpui::test]
    fn stopped_auto_shows_its_remembered_choice_without_waiting_for_playback(
        cx: &mut gpui::TestAppContext,
    ) {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let state = cx.new(|cx| {
            let mut state = UiState::new(core.clone(), cx);
            state.tracks = Arc::new(
                tracks
                    .iter()
                    .map(|id| core.library.get_track(*id).unwrap())
                    .collect(),
            );
            state
        });
        state.update(cx, |s, _| {
            s.select_automix_order(Mode::Shuffle);
            assert_eq!(s.order_button_mode(), Mode::Shuffle);
            assert_eq!(s.order_status_text(), None);
            core.mix_preparation.end_queue_session();
            s.automix_queue = core.mix_preparation.queue_view();
            assert_eq!(s.order_button_mode(), Mode::Shuffle);
            s.cycle_automix_order();
            assert_eq!(s.order_button_mode(), Mode::Best);
            assert_eq!(core.mix_preparation.queue_view().requested, Mode::Best);
        });
        core.shutting_down.store(true, Ordering::Release);
    }
}

impl UiState {
    pub(crate) fn order_status_text(&self) -> Option<String> {
        let queue = &self.automix_queue;
        if queue.error.is_some() {
            return Some("Best unavailable".into());
        }
        let tracks = if self.automix_active {
            self.automix_tracks.clone()
        } else {
            self.tracks.iter().map(|track| track.id).collect()
        };
        if let Some(progress) = self.core.mix_preparation.order_progress(&tracks) {
            return Some(format!("Best {:.0}%", progress * 100.));
        }
        if self.automix_active && queue.pending {
            return Some(
                if queue.requested == Mode::Best {
                    if self.core.mix_preparation.queue_best_ready() {
                        "Best pending"
                    } else {
                        "Calculating Best"
                    }
                } else {
                    "Order pending"
                }
                .into(),
            );
        }
        None
    }

    pub(crate) fn order_button_mode(&self) -> Mode {
        if self.automix_active {
            self.automix_queue.effective
        } else {
            self.automix_order_selection
                .choice()
                .unwrap_or(self.automix_queue.requested)
        }
    }
    pub(crate) fn showing_mix_playlist(&self) -> bool {
        self.automix_active && self.automix_playlist == self.playlist_sel
    }

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
        let queue = &self.automix_queue;
        let order: Vec<_> = queue.rows.iter().map(|entry| entry.track).collect();
        if order.is_empty() || order == ids {
            return self.tracks.clone();
        }
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
        self.select_automix_order_source(self.mix_playlist());
        if self.automix_active {
            self.automix_tracks = tracks.clone();
        }
        if self.automix_active && self.automix_order_selection.pending_cache && !tracks.is_empty() {
            self.automix_order_selection.pending_cache = false;
            crate::automix::prefer_cached_best(&self.core, tracks.clone());
        }
        crate::automix::prepare_playlist(&self.core, tracks);
    }

    pub(super) fn select_automix_order_source(&mut self, source: Option<i64>) {
        let Some(choice) = self.automix_order_selection.select(source) else {
            return;
        };
        self.automix_shuffle
            .store(matches!(choice, Some(Mode::Shuffle)), Ordering::Release);
        crate::automix::reset_order_for_playlist(
            &self.core,
            choice.map(|mode| matches!(mode, Mode::Best)),
        );
        if matches!(choice, Some(Mode::Shuffle)) {
            crate::automix::set_order_mode(&self.core, Mode::Shuffle, Vec::new());
        }
    }

    /// ORDER, SHUFFLE and BEST in turn. BEST plays the playlist in the order
    /// that mixes most smoothly, computed in the background.
    pub fn cycle_automix_order(&mut self) {
        let current = if self.automix_active {
            self.core.mix_preparation.queue_view().requested
        } else {
            self.order_button_mode()
        };
        self.select_automix_order(current.next());
    }

    pub fn select_automix_order(&mut self, mode: crate::automix::OrderMode) {
        let shuffle = mode == Mode::Shuffle;
        let best = mode == Mode::Best;
        self.automix_shuffle.store(shuffle, Ordering::Release);
        self.automix_order_selection
            .choose(self.mix_playlist(), mode);
        let tracks = if self.automix_active {
            self.automix_tracks.clone()
        } else {
            self.tracks.iter().map(|track| track.id).collect()
        };
        crate::automix::set_order_mode(&self.core, mode, tracks);
        self.automix_queue = self.core.mix_preparation.queue_view();
        if !best && self.automix_waiting {
            // Release a pending Best lookup immediately. Track analysis may
            // still be needed, but the optimizer cannot hold this mode switch.
            self.poll_automix();
        }
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
        self.select_automix_order_source(self.playlist_sel);
        self.automix_order_selection.pending_cache = false;
        // Prefer a cached Best order; a miss starts this session in playlist
        // order while Best warms independently. Explicit choices are retained.
        if self.automix_shuffle.load(Ordering::Acquire) {
            crate::automix::set_order_mode(&self.core, Mode::Shuffle, tracks.clone());
        } else if self.automix_order_selection.prefers_best() {
            crate::automix::prefer_cached_best(&self.core, tracks.clone());
        } else if !self.automix_shuffle.load(Ordering::Acquire) {
            crate::automix::default_to_best(&self.core, tracks.clone());
        }
        crate::automix::prepare_playlist(&self.core, tracks.clone());
        // Analysis and the cache lookup may still need to finish. The expensive
        // Best search never blocks startup.
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
        self.order_menu.open = false;
        self.core.mix_preparation.end_queue_session();
        self.automix_queue = self.core.mix_preparation.queue_view();
        self.dispatch(Command::StopAutomix);
        drop(_commit);
        self.automix_playlist = None;
        self.automix_tracks.clear();
        self.prepare_mix_playlist(self.tracks.iter().map(|track| track.id).collect());
    }

    pub(super) fn poll_automix(&mut self) -> bool {
        let mut changed = false;
        self.automix_queue = self.core.mix_preparation.queue_view();
        let order_status = self.order_status_text();
        if self.automix_order_status != order_status {
            self.automix_order_status = order_status;
            changed |= self.automix_active;
        }
        if let Some(rx) = self.automix_rx.take() {
            let mut finished = false;
            loop {
                let message = match rx.try_recv() {
                    Ok(message) => message,
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // A panicked worker has no Done message. Treat a closed
                        // channel as termination so AUTO can recover, rather
                        // than polling the same dead receiver forever.
                        tracing::error!("AutoMix worker disconnected before completion");
                        crate::automix::AutomixMsg::Done(Err("AutoMix worker disconnected".into()))
                    }
                };
                changed = true;
                match message {
                    crate::automix::AutomixMsg::Status(status) => {
                        self.automix_loading = false;
                        self.automix_preparation_progress = None;
                        self.automix_status = status;
                    }
                    crate::automix::AutomixMsg::Preparing(title) => {
                        self.automix_plan = None;
                        self.automix_loading = true;
                        self.automix_preparation_progress = None;
                        self.automix_status = format!("Loading {title}");
                    }
                    crate::automix::AutomixMsg::Planning(title) => {
                        self.automix_loading = true;
                        self.automix_preparation_progress = None;
                        self.automix_status = format!("Planning transition to {title}");
                    }
                    crate::automix::AutomixMsg::Plan(deck, plan, title) => {
                        tracing::trace!(?deck, pair = ?plan.summary.as_ref().map(|s| s.pair), "AutoMix worker staged plan");
                        self.automix_loading = false;
                        self.automix_preparation_progress = None;
                        self.automix_status = format!("Next: {title}");
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
                if finished {
                    break;
                }
            }
            if !finished {
                self.automix_rx = Some(rx);
            }
        }
        // The channel reports worker lifecycle, not playback identity. Read
        // the committed plan belonging to this exact engine snapshot.
        let current = self
            .snapshot
            .automix_on
            .then(|| self.core.engine.automix_plan(self.snapshot.automix_plan_id))
            .flatten();
        if self
            .automix_plan
            .as_ref()
            .map(|(d, p)| (*d, Arc::as_ptr(p)))
            != current.as_ref().map(|(d, p)| (*d, Arc::as_ptr(p)))
        {
            self.automix_plan = current;
            changed = true;
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
                "Preparing playback".into(),
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
