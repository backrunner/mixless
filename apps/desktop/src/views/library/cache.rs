//! Invalidate the cached library using the rows actually on screen.
use super::*;

impl UiState {
    fn mix_preview_visible(&self) -> bool {
        // Loading/resetting the silent deck after a completed handoff sets the
        // engine's manual-lane flags too. Those flags describe the old plan,
        // not the playback-owned forecast for the rest of the queue.
        self.showing_mix_playlist()
            && (!self.snapshot.automix_on || !self.snapshot.automix_modified)
    }

    pub(crate) fn shown_entries(&self, tracks: &[Track]) -> Vec<crate::automix::QueueEntry> {
        let queue = &self.automix_queue.rows;
        if self.showing_mix_playlist()
            && queue
                .iter()
                .map(|e| e.track)
                .eq(tracks.iter().map(|t| t.id))
        {
            return queue.as_ref().clone();
        }
        let mut counts = std::collections::HashMap::new();
        tracks
            .iter()
            .map(|t| {
                let occurrence = counts.entry(t.id).or_insert(0);
                let entry = crate::automix::QueueEntry {
                    track: t.id,
                    occurrence: *occurrence,
                };
                *occurrence += 1;
                entry
            })
            .collect()
    }
    pub(crate) fn queue_active_rows(&self, tracks: &[Track]) -> Option<(usize, usize)> {
        if !self.showing_mix_playlist() {
            return None;
        }
        let pair = self.automix_queue.pair?;
        let rows: Vec<_> = self
            .automix_queue
            .rows
            .iter()
            .copied()
            .filter(|entry| tracks.iter().any(|t| t.id == entry.track))
            .collect();
        if rows.iter().map(|e| e.track).eq(tracks.iter().map(|t| t.id)) {
            Some((
                rows.iter().position(|entry| *entry == pair[0])?,
                rows.iter().position(|entry| *entry == pair[1])?,
            ))
        } else {
            Some((
                tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.id == pair[0].track)
                    .nth(pair[0].occurrence)?
                    .0,
                tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.id == pair[1].track)
                    .nth(pair[1].occurrence)?
                    .0,
            ))
        }
    }
    pub(crate) fn library_render_key(&self, viewport_width: f32, scale: f32) -> LibraryKey {
        (
            // BEST can replace the displayed order without changing the source
            // playlist, including while both decks are paused.
            std::sync::Arc::as_ptr(&self.shown_tracks()) as usize,
            std::sync::Arc::as_ptr(&self.playlists) as usize,
            self.playlist_sel,
            (self.track_sel, self.track_sel_entry),
            self.focus,
            self.snapshot.decks.each_ref().map(|d| d.track_id),
            // Overview markers only invalidate the cached table after moving a
            // pixel. Deck waveforms still animate at display refresh rate.
            crate::wave::preview::positions(&self.snapshot.decks, self.presentation_frames)
                .map(|p| p.map(|p| (p * viewport_width.max(1.)).round() as u32)),
            (
                self.analysis_revision,
                self.mix_preview_visible(),
                self.automix_shuffle
                    .load(std::sync::atomic::Ordering::Relaxed),
                self.automix_queue.version,
            ),
            self.busy,
            self.picker_open,
            self.import_status(),
            // Paused decks also need fresh layout and waveform density when
            // resizing or moving between displays with different backing scales.
            (viewport_width.to_bits(), scale.to_bits()),
        )
    }

    pub(crate) fn library_mix_overlays(
        &self,
        tracks: &[Track],
    ) -> Vec<crate::wave::mix_overlay::Overlay> {
        // The armed pair and forecast come from one publication. Loading a
        // deck can clear the toolbar's plan, but must not blank these ranges
        // or overwrite a newer commit with a delayed UI message.
        let queue = &self.automix_queue;
        let preview = &queue.preview;
        let plans = preview.plans.as_slice();
        crate::wave::mix_overlay::rows_at(
            self.mix_preview_visible(),
            tracks,
            plans,
            preview.active.as_deref(),
            self.queue_active_rows(tracks),
        )
    }
}
