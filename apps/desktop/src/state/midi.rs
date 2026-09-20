//! MIDI performance controls and library browsing use the same UI paths as
//! mouse / keyboard input; library actions never enter the audio callback.
use super::*;
use mixless_midi::{MidiAction, MidiTarget, MidiValue};

fn stepped_index(current: Option<usize>, count: usize, steps: i16) -> Option<usize> {
    if count == 0 || steps == 0 {
        return current.filter(|index| *index < count);
    }
    let base = current
        .map(|i| i.min(count - 1) as i64)
        .unwrap_or(if steps > 0 { -1 } else { count as i64 });
    Some((base + steps as i64).clamp(0, count as i64 - 1) as usize)
}

impl UiState {
    pub(super) fn apply_midi(&mut self, action: MidiAction) {
        use MidiTarget::*;
        let snapshot = self.core.engine.snapshot();
        // FX editing uses the mirrored values, so refresh them before every
        // event, including several encoder ticks in the same display frame.
        self.fx = snapshot.decks.each_ref().map(|d| d.fx);
        match action.target {
            Play { deck } => self.play_pause(deck),
            Sync { deck } => self.sync(deck),
            Cue { deck, index } => self.trigger_cue(deck, index as usize, false),
            ClearCue { deck, index } => self.clear_cue(deck, index as usize),
            TemporaryCue { deck } => self.temporary_cue(deck, false),
            FxOn { deck, slot } => self.toggle_fx(deck, slot as usize),
            FxPrevious { deck, slot } => self.cycle_fx(deck, slot as usize, -1),
            FxNext { deck, slot } => self.cycle_fx(deck, slot as usize, 1),
            Automix => self.toggle_automix(),
            AutomixPause if self.automix_active => self.dispatch(if snapshot.automix_paused {
                Command::ResumeAutomix
            } else {
                Command::PauseAutomix
            }),
            AutomixSkip if self.automix_active => self.dispatch(Command::SkipAutomix),
            AutomixShuffle => {
                self.automix_shuffle.fetch_xor(true, Ordering::AcqRel);
            }
            BrowseTracks => self.midi_browse_tracks(action.value.steps()),
            TrackPrevious => self.midi_browse_tracks(-1),
            TrackNext => self.midi_browse_tracks(1),
            BrowsePlaylists => self.midi_browse_playlists(action.value.steps()),
            PlaylistPrevious => self.midi_browse_playlists(-1),
            PlaylistNext => self.midi_browse_playlists(1),
            LoadSelected { deck } => self.midi_load_selected(deck),
            LoadFocused => self.midi_load_selected(self.focus),
            Focus { deck } => self.focus = deck,
            _ => {
                if let Some(command) = action.command(&snapshot) {
                    self.dispatch(command);
                }
            }
        }
    }

    fn midi_browse_tracks(&mut self, steps: i16) {
        let current = self
            .midi_track_cursor
            .filter(|(index, id)| {
                self.track_sel == Some(id.0)
                    && self.tracks.get(*index).is_some_and(|track| track.id == *id)
            })
            .map(|(index, _)| index)
            .or_else(|| {
                self.tracks
                    .iter()
                    .position(|track| Some(track.id.0) == self.track_sel)
            });
        let Some(index) = stepped_index(current, self.tracks.len(), steps) else {
            return;
        };
        self.track_menu = None;
        self.track_sel = Some(self.tracks[index].id.0);
        self.midi_track_cursor = Some((index, self.tracks[index].id));
        let rows =
            crate::views::library::import_rows::rows(self.tracks.len(), &self.playlist_imports);
        let row = rows
            .iter()
            .position(|row| *row == crate::views::library::import_rows::Row::Track(index))
            .unwrap_or(index);
        self.track_scroll
            .scroll_to_item(row, gpui::ScrollStrategy::Top);
        self.library_key = None;
    }

    fn midi_browse_playlists(&mut self, steps: i16) {
        let sources = crate::views::library::playlist_sources(&self.playlists);
        let mut selections = vec![(None, None)]; // All Tracks precedes the groups.
        for (row, source) in sources.iter().enumerate() {
            if let crate::views::library::SourceEntry::Playlist(index) = *source {
                selections.push((Some(self.playlists[index].id), Some(row)));
            }
        }
        let current = selections
            .iter()
            .position(|(id, _)| *id == self.playlist_sel);
        let Some(index) = stepped_index(current, selections.len(), steps) else {
            return;
        };
        let (selection, row) = selections[index];
        if selection != self.playlist_sel {
            self.select_playlist(selection);
        }
        if let Some(row) = row {
            self.playlist_scroll
                .scroll_to_item(row, gpui::ScrollStrategy::Top);
            self.library_key = None;
        }
    }

    fn midi_load_selected(&mut self, deck: DeckId) {
        if let Some(track) = self
            .tracks
            .iter()
            .find(|t| Some(t.id.0) == self.track_sel)
            .map(|t| t.id)
        {
            self.focus = deck;
            self.load_deck(deck, track);
        }
    }
}

/// Preserve event ordering while combining adjacent encoder ticks. Loading
/// after browsing still uses the newly selected item.
pub(super) fn coalesce(actions: Vec<MidiAction>) -> Vec<MidiAction> {
    let mut result: Vec<MidiAction> = Vec::with_capacity(actions.len());
    for action in actions {
        if matches!(
            action.target,
            MidiTarget::BrowseTracks | MidiTarget::BrowsePlaylists
        ) {
            if let Some(previous) = result.last_mut().filter(|p| p.target == action.target) {
                if let (MidiValue::Relative(a), MidiValue::Relative(b)) =
                    (&mut previous.value, action.value)
                {
                    // Only combine ticks travelling in the same direction: at
                    // a list boundary, +1 then -1 is not equivalent to zero.
                    if a.signum() == b.signum() {
                        *a = a.saturating_add(b);
                        continue;
                    }
                }
            }
        }
        result.push(action);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browsing_selects_visible_order_and_clamps_at_both_ends() {
        assert_eq!(stepped_index(None, 0, 1), None);
        assert_eq!(stepped_index(None, 4, 1), Some(0));
        assert_eq!(stepped_index(None, 4, -1), Some(3));
        assert_eq!(stepped_index(Some(0), 4, -63), Some(0));
        assert_eq!(stepped_index(Some(2), 4, 63), Some(3));
        assert_eq!(stepped_index(Some(1), 4, 2), Some(3));
    }
    #[test]
    fn encoder_coalescing_preserves_load_order_and_reversals() {
        let tick = |n| MidiAction {
            target: MidiTarget::BrowseTracks,
            value: MidiValue::Relative(n),
        };
        let load = MidiAction {
            target: MidiTarget::LoadFocused,
            value: MidiValue::Press,
        };
        assert_eq!(
            coalesce(vec![tick(1), tick(2), tick(-1), load.clone(), tick(1)]),
            vec![tick(3), tick(-1), load, tick(1)]
        );
    }
}
