//! Deck transport, synchronization and performance controls.
use super::*;

impl UiState {
    pub fn deck(&self, id: DeckId) -> &mixless_protocol::DeckSnapshot {
        &self.snapshot.decks[id.index()]
    }

    /* ---- engine commands ------------------------------------------------ */

    pub fn dispatch(&mut self, cmd: Command) {
        if self.automix_active
            && matches!(
                cmd,
                Command::PlayPause { .. }
                    | Command::JumpCue { .. }
                    | Command::TriggerTemporaryCue { .. }
                    | Command::BeginCuePreview { .. }
                    | Command::SetBrake { on: true, .. }
            )
        {
            self.stop_automix();
        }
        if self
            .sync_request
            .as_ref()
            .is_some_and(|r| r.cancelled_by(&cmd))
        {
            self.sync_request = None;
        }
        if let Err(e) = self.core.engine.dispatch(cmd) {
            self.error = e.to_string().into();
        }
    }

    pub fn play_pause(&mut self, deck: DeckId) {
        if self.core.engine.snapshot().deck(deck).frames == 0 {
            return;
        }
        if self.automix_active {
            self.stop_automix();
        }
        self.dispatch(Command::PlayPause { deck });
    }

    pub fn grid_pending(&self, deck: DeckId) -> bool {
        self.grid_rx[deck.index()].is_some()
    }

    pub fn sync(&mut self, deck: DeckId) {
        let snapshot = self.core.engine.snapshot();
        let d = snapshot.deck(deck);
        self.error = "".into();
        if self.sync_waiting(deck) {
            self.sync_request = None;
            return;
        }
        self.sync_request = None;
        if d.synced {
            self.dispatch(Command::DisableSync { deck });
            return;
        }
        // Sync takes over timing without switching off playlist automation.
        if let Some(request) = crate::beat_sync::SyncRequest::new(deck, &snapshot) {
            self.sync_request = Some(request);
        } else {
            self.error = "Load two tracks and turn off reverse before syncing".into();
        }
    }

    pub fn sync_waiting(&self, deck: DeckId) -> bool {
        self.sync_request
            .as_ref()
            .is_some_and(|r| r.follower == deck)
    }

    pub fn jump_cue(&mut self, deck: DeckId, index: usize) {
        if self.automix_active {
            self.stop_automix();
        }
        self.dispatch(Command::JumpCue {
            deck,
            index: index as u8,
        });
    }

    /// Empty pads set a cue; populated pads jump without changing playback.
    pub fn trigger_cue(&mut self, deck: DeckId, index: usize, shift: bool) {
        let snapshot = self.core.engine.snapshot();
        let d = &snapshot.decks[deck.index()];
        if index >= d.cues.len() || d.track_id.is_none() {
            return;
        }
        if shift || self.cue_shift[deck.index()] {
            if d.cues[index].is_none() {
                self.set_cue_now(deck, index);
            }
            self.cue_role_editor[deck.index()] = Some((index, d.track_id.unwrap()));
            self.cue_shift[deck.index()] = false;
        } else if d.cues[index].is_none() {
            self.set_cue_now(deck, index);
        } else {
            self.jump_cue(deck, index);
        }
    }

    pub fn set_loop(&mut self, deck: DeckId, beats: f32, on: bool) {
        self.dispatch(Command::SetLoopBeats { deck, beats, on });
    }

    pub fn set_pfl(&mut self, deck: DeckId, on: bool) {
        self.dispatch(Command::SetPfl { deck, on });
    }

    pub fn set_kill(&mut self, deck: DeckId, band: EqBand, on: bool) {
        self.dispatch(Command::SetEqKill { deck, band, on });
    }

    pub fn cycle_fx(&mut self, deck: DeckId, slot: usize, dir: i32) {
        let kinds = FxKind::ALL;
        let idx = kinds
            .iter()
            .position(|k| *k == self.fx[deck.index()][slot].kind)
            .unwrap_or(0);
        let next = kinds[(idx as i32 + dir).rem_euclid(kinds.len() as i32) as usize];
        self.select_fx(deck, slot, next);
    }

    pub fn toggle_fx(&mut self, deck: DeckId, slot: usize) {
        self.fx[deck.index()][slot].on = !self.fx[deck.index()][slot].on;
        let on = self.fx[deck.index()][slot].on;
        if on {
            self.apply_fx(deck, slot);
        }
        self.dispatch(Command::SetFxBypass {
            deck,
            slot: fx_slot(slot),
            on: !on,
        });
    }

    /// Apply an FX slot only while the pointer is held down. The previous
    /// latched state is restored on release, including releases outside the
    /// button handled by the root capture listener.
    pub fn begin_momentary_fx(&mut self, deck: DeckId, slot: usize) {
        if self.momentary_fx.is_some() {
            return;
        }
        let was_on = self.fx[deck.index()][slot].on;
        self.fx[deck.index()][slot].on = true;
        if !was_on {
            self.apply_fx(deck, slot);
        }
        self.dispatch(Command::SetFxBypass {
            deck,
            slot: fx_slot(slot),
            on: false,
        });
        self.momentary_fx = Some((deck, slot, was_on));
    }

    /// Stop a held FX and restore its persistent toggle. Returns whether a
    /// held FX was released so callers can request one repaint.
    pub fn end_momentary_fx(&mut self) -> bool {
        let Some((deck, slot, was_on)) = self.momentary_fx.take() else {
            return false;
        };
        if !was_on {
            self.fx[deck.index()][slot].on = false;
            self.dispatch(Command::SetFxBypass {
                deck,
                slot: fx_slot(slot),
                on: true,
            });
        }
        true
    }

    pub fn momentary_fx_active(&self, deck: DeckId, slot: usize) -> bool {
        self.momentary_fx
            .map(|(held_deck, held_slot, _)| held_deck == deck && held_slot == slot)
            .unwrap_or(false)
    }

    pub fn set_fx_mix(&mut self, deck: DeckId, slot: usize, mix: f32) {
        self.fx[deck.index()][slot].mix = mix;
        self.apply_fx(deck, slot);
    }
}
