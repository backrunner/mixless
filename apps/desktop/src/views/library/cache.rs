//! Invalidate the cached library using the rows actually on screen.
use super::*;

impl UiState {
    pub(crate) fn library_render_key(&self, viewport_width: f32, scale: f32) -> LibraryKey {
        (
            // BEST can replace the displayed order without changing the source
            // playlist, including while both decks are paused.
            std::sync::Arc::as_ptr(&self.shown_tracks()) as usize,
            std::sync::Arc::as_ptr(&self.playlists) as usize,
            self.playlist_sel,
            self.track_sel,
            self.focus,
            self.snapshot.decks.each_ref().map(|d| d.track_id),
            // Overview markers only invalidate the cached table after moving a
            // pixel. Deck waveforms still animate at display refresh rate.
            crate::wave::preview::positions(&self.snapshot.decks, self.presentation_frames)
                .map(|p| p.map(|p| (p * viewport_width.max(1.)).round() as u32)),
            (
                self.analysis_revision,
                self.automix_active,
                self.automix_shuffle
                    .load(std::sync::atomic::Ordering::Relaxed),
                self.core
                    .mix_preparation
                    .preview_revision
                    .load(std::sync::atomic::Ordering::Acquire),
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
        let preview = self.core.mix_preparation.previews();
        let plans = if self
            .automix_shuffle
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            &[][..]
        } else {
            preview.plans.as_slice()
        };
        crate::wave::mix_overlay::rows(
            self.automix_active,
            tracks,
            plans,
            preview.active.as_deref(),
        )
    }
}
