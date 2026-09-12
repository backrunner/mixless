//! Mouse gestures keep cue jumps on press, short stops on release and brake
//! holds distinct. A load or window deactivation cancels the pending gesture.
use super::*;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransportButton {
    Play,
    Cue,
}

pub(super) struct TransportPress {
    deck: DeckId,
    track: TrackId,
    button: TransportButton,
    started: Instant,
    was_playing: bool,
    handled: bool,
}

impl TransportPress {
    fn hold_due(&self, now: Instant) -> bool {
        !self.handled && now.duration_since(self.started) >= Duration::from_millis(400)
    }
    fn short_stop(&self) -> bool {
        self.button == TransportButton::Play && self.was_playing && !self.handled
    }
}

impl UiState {
    pub fn begin_transport_press(&mut self, deck: DeckId, button: TransportButton) {
        self.cancel_transport_press();
        let snapshot = self.core.engine.snapshot();
        let d = snapshot.deck(deck);
        let Some(track) = d.track_id.filter(|_| d.frames > 0) else {
            return;
        };
        let handled = match button {
            TransportButton::Cue => {
                self.temporary_cue(deck, false);
                false
            }
            TransportButton::Play if !d.playing || d.brake => {
                self.play_pause(deck);
                true
            }
            TransportButton::Play => false,
        };
        self.transport_press = Some(TransportPress {
            deck,
            track,
            button,
            started: Instant::now(),
            was_playing: d.playing,
            handled,
        });
    }
    pub fn temporary_cue(&mut self, deck: DeckId, clear: bool) {
        if !clear && self.automix_active {
            self.stop_automix();
        }
        self.dispatch(if clear {
            Command::ClearTemporaryCue { deck }
        } else {
            Command::TriggerTemporaryCue { deck }
        });
    }
    pub(super) fn poll_transport_press(&mut self) -> bool {
        let Some(press) = self.transport_press.as_ref() else {
            return false;
        };
        if self.core.engine.snapshot().deck(press.deck).track_id != Some(press.track) {
            self.transport_press = None;
            return true;
        }
        if !press.hold_due(Instant::now()) {
            return false;
        }
        let (deck, button) = (press.deck, press.button);
        self.transport_press.as_mut().unwrap().handled = true;
        match button {
            TransportButton::Cue => self.temporary_cue(deck, true),
            TransportButton::Play => {
                if self.automix_active {
                    self.stop_automix();
                }
                self.dispatch(Command::SetBrake { deck, on: true });
            }
        }
        true
    }
    pub fn end_transport_press(&mut self) -> bool {
        // Resolve a hold even when the release lands between display frames.
        self.poll_transport_press();
        let Some(press) = self.transport_press.take() else {
            return false;
        };
        if press.short_stop()
            && self.core.engine.snapshot().deck(press.deck).track_id == Some(press.track)
        {
            // If EOF was reached during the press, release must not restart it.
            if self.core.engine.snapshot().deck(press.deck).playing {
                self.play_pause(press.deck);
            }
        }
        true
    }
    pub fn cancel_transport_press(&mut self) {
        self.transport_press = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_release_and_hold_are_exclusive_and_initial_start_does_not_brake() {
        let started = Instant::now();
        let mut p = TransportPress {
            deck: DeckId::A,
            track: TrackId(1),
            button: TransportButton::Play,
            started,
            was_playing: true,
            handled: false,
        };
        assert!(p.short_stop());
        assert!(!p.hold_due(started + Duration::from_millis(399)));
        assert!(p.hold_due(started + Duration::from_millis(400)));
        p.handled = true;
        assert!(!p.short_stop());
        assert!(!p.hold_due(started + Duration::from_secs(1)));
        p.was_playing = false;
        assert!(!p.short_stop());
        p.button = TransportButton::Cue;
        p.handled = false;
        assert!(!p.short_stop());
        assert!(p.hold_due(started + Duration::from_secs(1)));
    }
}
