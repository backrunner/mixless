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
    braking: bool,
}

impl TransportPress {
    fn hold_due(&self, now: Instant) -> bool {
        !self.handled && now.duration_since(self.started) >= Duration::from_millis(400)
    }
    fn release_command(&self, current: &mixless_protocol::DeckSnapshot) -> Option<Command> {
        if current.track_id != Some(self.track) {
            return None;
        }
        if self.braking {
            Some(Command::SetBrake {
                deck: self.deck,
                on: false,
            })
        } else if self.short_stop() && current.playing {
            Some(Command::PlayPause { deck: self.deck })
        } else {
            None
        }
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
            braking: false,
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
                self.transport_press.as_mut().unwrap().braking = true;
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
        if let Some(command) = press.release_command(self.core.engine.snapshot().deck(press.deck)) {
            self.dispatch(command);
        }
        true
    }
    pub fn cancel_transport_press(&mut self) {
        if let Some(press) = self.transport_press.take() {
            // Focus loss/new gesture must release an active hold without
            // turning an unhandled short click into an accidental stop.
            if press.braking {
                if let Some(command) =
                    press.release_command(self.core.engine.snapshot().deck(press.deck))
                {
                    self.dispatch(command);
                }
            }
        }
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
            braking: false,
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
    #[test]
    fn releasing_a_hold_stops_only_the_original_loaded_track() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        crate::analysis::load(&core, DeckId::A, tracks[0], || true).unwrap();
        let mut p = TransportPress {
            deck: DeckId::A,
            track: tracks[0],
            button: TransportButton::Play,
            started: Instant::now(),
            was_playing: true,
            handled: true,
            braking: true,
        };
        let mut d = core.engine.snapshot().decks[0].clone();
        d.playing = true;
        assert!(matches!(
            p.release_command(&d),
            Some(Command::SetBrake {
                deck: DeckId::A,
                on: false
            })
        ));
        d.playing = false;
        assert!(matches!(
            p.release_command(&d),
            Some(Command::SetBrake { on: false, .. })
        ));
        d.track_id = Some(tracks[1]);
        assert!(p.release_command(&d).is_none());
        d.track_id = Some(tracks[0]);
        d.playing = true;
        p.braking = false;
        // A press that started playback does not stop it on release.
        assert!(p.release_command(&d).is_none());
        p.handled = false;
        assert!(matches!(
            p.release_command(&d),
            Some(Command::PlayPause { deck: DeckId::A })
        ));
        d.playing = false;
        assert!(p.release_command(&d).is_none());
    }
}
