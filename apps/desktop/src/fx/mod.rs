//! Effect selection and edit actions. Rendering and parameter mapping are separate.

mod bar;
mod editor;
mod params;

pub use params::FxParam;

use mixless_protocol::{DeckId, FX_BEATS, FxKind, FxState};

use crate::state::UiState;

impl UiState {
    pub fn select_fx(&mut self, deck: DeckId, slot: usize, kind: FxKind) {
        self.end_drag();
        self.end_momentary_fx();
        let current = self.fx[deck.index()][slot];
        if current.kind != kind {
            self.fx[deck.index()][slot] = FxState {
                on: current.on,
                mix: current.mix,
                ..FxState::new(kind)
            };
            self.apply_fx(deck, slot);
        }
    }

    pub fn set_fx_beats(&mut self, deck: DeckId, slot: usize, beats: f32) {
        let fx = &mut self.fx[deck.index()][slot];
        if FX_BEATS.contains(&beats) && beats <= fx.kind.max_beats() {
            fx.beats = beats;
            self.apply_fx(deck, slot);
        }
    }

    pub fn set_fx_sync(&mut self, deck: DeckId, slot: usize, synced: bool) {
        let bpm = self.deck(deck).sounding_bpm;
        let fx = &mut self.fx[deck.index()][slot];
        fx.rate_hz = if synced {
            0.0
        } else {
            ((if bpm > 0.0 { bpm } else { 120.0 }) / (60.0 * fx.beats)).clamp(0.05, 20.0)
        };
        self.apply_fx(deck, slot);
    }

    pub fn close_fx_editor(&mut self) {
        self.end_drag();
        self.end_momentary_fx();
        self.fx_editor = None;
    }
}
