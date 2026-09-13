//! Persisted hot cues and per-track entry/exit anchors.
use super::*;
impl UiState {
    pub fn set_cue_now(&mut self, deck: DeckId, index: usize) {
        self.save_cue(deck, index, CueKind::Hot);
    }
    pub fn toggle_cue_shift(&mut self, deck: DeckId) {
        self.cue_shift[deck.index()] = !self.cue_shift[deck.index()];
        self.cue_role_editor[deck.index()] = None;
    }
    pub fn assign_cue_role(&mut self, deck: DeckId, kind: CueKind) {
        let Some((index, expected)) = self.cue_role_editor[deck.index()].take() else {
            return;
        };
        let snapshot = self.core.engine.snapshot();
        let d = snapshot.deck(deck);
        if d.track_id != Some(expected) {
            return;
        }
        if let Some(frame) = d.cues[index] {
            self.persist_cue(deck, index, frame, kind, expected);
        }
    }
    fn save_cue(&mut self, deck: DeckId, index: usize, kind: CueKind) {
        let snapshot = self.core.engine.snapshot();
        let d = snapshot.deck(deck);
        if let Some(id) = d.track_id {
            self.persist_cue(deck, index, d.frame, kind, id);
        }
    }
    fn persist_cue(&mut self, deck: DeckId, index: usize, frame: u64, kind: CueKind, id: TrackId) {
        let _commit = self.core.automix_commit.lock().expect("cue commit");
        let snapshot = self.core.engine.snapshot();
        if index >= 8 || snapshot.deck(deck).track_id != Some(id) {
            return;
        }
        if let Err(e) = self
            .core
            .library
            .set_cue(id, index as u8, frame, kind, true)
        {
            self.error = format!("Could not save cue {}: {e}", index + 1).into();
            return;
        }
        for deck in [DeckId::A, DeckId::B] {
            if snapshot.decks[deck.index()].track_id == Some(id) {
                self.core.engine.set_cue_frame(deck, index as u8, frame);
                let _ = self.core.engine.dispatch(Command::SetCueKind {
                    track_id: id,
                    index: index as u8,
                    kind,
                });
            }
        }
        self.library_previews.invalidate(id);
        crate::automix::refresh_previews(&self.core);
        self.library_key = None;
    }
    pub fn clear_cue(&mut self, deck: DeckId, index: usize) {
        let _commit = self.core.automix_commit.lock().expect("cue commit");
        let snapshot = self.core.engine.snapshot();
        let Some(id) = snapshot.decks[deck.index()].track_id else {
            return;
        };
        if index >= 8 {
            return;
        }
        if let Err(e) = self.core.library.clear_cue(id, index as u8) {
            self.error = format!("Could not clear cue: {e}").into();
            return;
        }
        for deck in [DeckId::A, DeckId::B] {
            if snapshot.decks[deck.index()].track_id == Some(id) {
                let _ = self.core.engine.dispatch(Command::ClearCue {
                    deck,
                    index: index as u8,
                });
            }
        }
        self.library_previews.invalidate(id);
        crate::automix::refresh_previews(&self.core);
        self.library_key = None;
    }
}
