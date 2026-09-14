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
        match play_target(
            self.core.engine.snapshot().deck(deck).frames,
            self.deck_loading[deck.index()].is_some(),
        ) {
            PlayTarget::Skip => return,
            PlayTarget::Queued => {
                if self.automix_active {
                    self.stop_automix();
                }
                self.pending_play[deck.index()] = !self.pending_play[deck.index()];
                return;
            }
            PlayTarget::Now => {}
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

/// Where a Play press goes. A press during a deck load would be dropped by the
/// engine (`frames == 0` early-return) or undone by the commit's `playing`
/// reset, so it is queued until the load resolves instead.
pub(super) enum PlayTarget {
    Now,
    Queued,
    Skip,
}

pub(super) fn play_target(frames: u64, loading: bool) -> PlayTarget {
    if frames > 0 {
        PlayTarget::Now
    } else if loading {
        PlayTarget::Queued
    } else {
        PlayTarget::Skip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_queued_while_loading_starts_the_committed_deck() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        assert_eq!(core.engine.snapshot().deck(DeckId::A).frames, 0);
        let mut pending_play = [false; 2];
        // The press lands while the load worker still owns the deck.
        assert!(matches!(play_target(0, true), PlayTarget::Queued));
        pending_play[0] = !pending_play[0];
        assert!(pending_play[0]);
        // Pressing again before the commit cancels the queued press.
        pending_play[0] = !pending_play[0];
        assert!(!pending_play[0]);
        pending_play[0] = true;
        // A press on an idle empty deck still does nothing.
        assert!(matches!(play_target(0, false), PlayTarget::Skip));
        // A loaded deck mid-swap toggles its current buffer immediately.
        assert!(matches!(play_target(48000, true), PlayTarget::Now));
        crate::analysis::load_manual(&core, DeckId::A, tracks[0], || true).unwrap();
        assert!(matches!(
            play_target(core.engine.snapshot().deck(DeckId::A).frames, false),
            PlayTarget::Now
        ));
        // poll() drains the flag into a real PlayPause once the load commits.
        if std::mem::take(&mut pending_play[0]) {
            core.engine
                .dispatch(Command::PlayPause { deck: DeckId::A })
                .unwrap();
        }
        let snapshot = core.engine.snapshot();
        assert!(snapshot.decks[0].playing);
        assert!(
            core.engine
                .render_offline(480)
                .iter()
                .any(|v| v.abs() > 0.01)
        );
    }
}
