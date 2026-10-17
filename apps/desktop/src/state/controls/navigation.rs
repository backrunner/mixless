//! Display preferences and audible track navigation.
use super::*;
use crate::controls::{PlatterGeometry, ProgressDrag};

impl UiState {
    pub fn toggle_deck_time(&mut self, deck: DeckId) {
        let elapsed = !self.deck_elapsed[deck.index()];
        match self
            .core
            .settings
            .update(|s| s.deck_elapsed[deck.index()] = elapsed)
        {
            Ok(()) => self.deck_elapsed[deck.index()] = elapsed,
            Err(error) => self.error = error.into(),
        }
    }

    pub(crate) fn begin_progress(
        &mut self,
        deck: DeckId,
        x: f32,
        y: f32,
        geometry: PlatterGeometry,
    ) {
        self.end_drag();
        if self.deck(deck).frames == 0 {
            return;
        }
        if let Some(drag) = ProgressDrag::new(geometry, x, y) {
            self.focus = deck;
            self.dispatch(Command::SetJogTouch {
                deck,
                touching: true,
            });
            self.seek_progress(deck, drag.position);
            self.drag = Some(DragCtl::Progress { deck, drag });
        }
    }

    pub(super) fn seek_progress(&mut self, deck: DeckId, position: f64) {
        let frame = (position.clamp(0., 1.) * self.deck(deck).frames.saturating_sub(1) as f64)
            .round() as u64;
        self.dispatch(Command::ScrubTo { deck, frame });
    }

    pub fn scroll_wave(&mut self, deck: DeckId, pixels: f32, frames_per_px: f32) {
        if self.drag.is_some() || self.deck(deck).frames == 0 {
            return;
        }
        let delta_frames = -pixels * frames_per_px;
        if delta_frames.is_finite() && delta_frames.abs() >= 0.01 {
            self.focus = deck;
            if self.wheel_scrub.is_none_or(|(active, _)| active != deck) {
                self.end_wheel_scrub();
                self.dispatch(Command::SetJogTouch {
                    deck,
                    touching: true,
                });
            }
            self.dispatch(Command::Jog { deck, delta_frames });
            self.wheel_scrub = Some((deck, std::time::Instant::now()));
        }
    }

    pub(crate) fn end_wheel_scrub(&mut self) {
        if let Some((deck, _)) = self.wheel_scrub.take() {
            self.dispatch(Command::SetJogTouch {
                deck,
                touching: false,
            });
        }
    }

    pub(crate) fn poll_wheel_scrub(&mut self) -> bool {
        if self
            .wheel_scrub
            .is_some_and(|(_, at)| at.elapsed().as_millis() >= 120)
        {
            self.end_wheel_scrub();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests;
