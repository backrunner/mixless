use super::*;

impl UiState {
    pub fn remove_playlist_track(&mut self, playlist: i64, track: TrackId) {
        let core = self.core.clone();
        let (tx, rx) = channel();
        self.library_actions.push(rx);
        std::thread::spawn(move || {
            let result = core
                .library
                .remove_playlist_track(PlaylistId(playlist), track)
                .map(|_| track)
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    pub fn reanalyze_track(&mut self, track: TrackId, clear_cues: bool) {
        self.core.analysis.queue_reanalysis(track);
        let core = self.core.clone();
        let (tx, rx) = channel();
        self.library_actions.push(rx);
        std::thread::spawn(move || {
            let result = crate::analysis::reanalyze(&core, track, clear_cues).map(|_| track);
            let _ = tx.send(result);
        });
    }

    pub(super) fn poll_library_actions(&mut self) -> bool {
        let mut changed = false;
        let pending = std::mem::take(&mut self.library_actions);
        for rx in pending {
            match rx.try_recv() {
                Ok(Ok(track)) => {
                    self.library_previews.invalidate(track);
                    crate::automix::refresh_previews(&self.core);
                    for i in 0..2 {
                        if self.snapshot.decks[i].track_id == Some(track) {
                            self.wave_tempo[i] = None;
                        }
                    }
                    self.refresh_tracks();
                    changed = true;
                }
                Ok(Err(error)) => {
                    self.error = error.into();
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.library_actions.push(rx),
                Err(_) => {}
            }
        }
        changed
    }
}
