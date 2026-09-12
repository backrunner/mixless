//! Playlist selection and cancellable deck preparation.
use super::*;

impl UiState {
    pub fn select_playlist(&mut self, id: Option<i64>) {
        self.library_refresh.initial = false;
        self.track_menu = None;
        self.playlist_sel = id;
        self.track_sel = None;
        self.tracks = Arc::new(Vec::new());
        self.track_scroll
            .scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
        self.refresh_tracks();
    }

    pub fn load_deck(&mut self, deck: DeckId, track_id: TrackId) {
        self.sync_request = None;
        if self.automix_active {
            self.automix_epoch.fetch_add(1, Ordering::AcqRel);
            self.automix_rx = None;
            self.automix_plan = None;
            self.automix_retry = Some(std::time::Instant::now());
            self.dispatch(Command::StopAutomix);
        }
        let index = deck.index();
        let generation = self.deck_load_epoch[index].fetch_add(1, Ordering::AcqRel) + 1;
        let epoch = self.deck_load_epoch.clone();
        let core = self.core.clone();
        let (tx, rx) = channel();
        let (grid_tx, grid_rx) = channel();
        self.deck_load_rx[index] = Some(rx);
        self.grid_rx[index] = Some(grid_rx);
        self.error = "".into();
        std::thread::spawn(move || {
            let active = || epoch[index].load(Ordering::Acquire) == generation;
            let loaded_hash = match crate::analysis::load_manual(&core, deck, track_id, &active) {
                Ok(Some(hash)) => {
                    let _ = tx.send(Ok(()));
                    hash
                }
                Ok(None) => return,
                Err(error) => {
                    let _ = tx.send(Err(error));
                    return;
                }
            };
            let grid = (|| {
                let prepared = crate::analysis::prepare(&core, track_id)?;
                if prepared.track.content_hash != loaded_hash {
                    return Err("File changed after loading; reload the track".into());
                }
                let _commit = core
                    .automix_commit
                    .lock()
                    .map_err(|_| "Deck load lock poisoned")?;
                if !active() {
                    return Err("Load replaced".into());
                }
                if prepared.analysis.tempo.beats.len() >= 2 {
                    core.engine
                        .set_beat_grid(deck, track_id, prepared.analysis.tempo.clone())
                        .map_err(|e| e.to_string())?;
                }
                core.engine
                    .set_bpm(deck, prepared.analysis.tempo.global_bpm);
                for cue in core.library.cues(track_id).map_err(|e| e.to_string())? {
                    core.engine.set_cue_frame(deck, cue.index, cue.frame);
                    let _ = core.engine.dispatch(mixless_protocol::Command::SetCueKind {
                        track_id,
                        index: cue.index,
                        kind: if cue.user_set {
                            cue.kind
                        } else {
                            mixless_protocol::CueKind::Hot
                        },
                    });
                }
                core.analysis.loaded.lock().expect("loaded analysis")[index] =
                    Some(prepared.clone());
                Ok(prepared.analysis.tempo.clone())
            })();
            if active() {
                let _ = grid_tx.send(grid);
            }
        });
    }
}
