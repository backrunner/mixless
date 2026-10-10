//! Hardware performance dispatch shares the engine's typed commands with mouse/keyboard.
//! Library and persistence operations wake GPUI; audio controls never wait for a paint.
use super::*;
use mixless_controllers::{ControlEvent, MidiTarget, MidiValue};
impl AppCore {
    pub fn attach_controller_runtime(self: &Arc<Self>) {
        let Some(hub) = self.midi.lock().expect("controllers").clone() else {
            return;
        };
        let weak = Arc::downgrade(self);
        let reader = weak.clone();
        hub.set_handler(
            Arc::new(move |event| {
                weak.upgrade()
                    .is_some_and(|core| core.apply_controller_event(event))
            }),
            Arc::new(move || {
                reader
                    .upgrade()
                    .map_or_else(Default::default, |core| core.engine.snapshot())
            }),
        );
        if let Some(wake) = self
            .controller_wake
            .lock()
            .expect("controller wake")
            .clone()
        {
            hub.set_notify(wake);
        }
    }
    fn apply_controller_event(&self, event: ControlEvent) -> bool {
        use MidiTarget::*;
        let snapshot = self.engine.snapshot();
        let action = event.action;
        match action.target {
            LoadSelected { deck } => {
                self.controller_loading[deck.index()].store(true, Ordering::Release);
                return false;
            }
            LoadFocused => {
                for loading in &self.controller_loading {
                    loading.store(true, Ordering::Release);
                }
                return false;
            }
            Play { deck } | Sync { deck } | Cue { deck, .. } | TemporaryCue { deck }
                if self.controller_loading[deck.index()].load(Ordering::Acquire) =>
            {
                return false;
            }
            _ => {}
        }
        let command = match action.target {
            Cue { deck, .. } if action.value == MidiValue::Release => {
                Some(Command::EndCuePreview { deck })
            }
            Cue { deck, index }
                if action.value == MidiValue::Hold
                    && snapshot
                        .deck(deck)
                        .cues
                        .get(index as usize)
                        .is_some_and(|c| c.is_some()) =>
            {
                Some(Command::BeginCuePreview {
                    deck,
                    frame: snapshot.deck(deck).cues[index as usize].unwrap(),
                })
            }
            TemporaryCue { deck } if action.value == MidiValue::Hold => {
                Some(Command::BeginCuePreview {
                    deck,
                    frame: snapshot
                        .deck(deck)
                        .temporary_cue_frame
                        .unwrap_or(snapshot.deck(deck).frame),
                })
            }
            Play { deck }
                if snapshot.deck(deck).frames > 0
                    && !self.controller_loading[deck.index()].load(Ordering::Acquire) =>
            {
                Some(Command::PlayPause { deck })
            }
            TemporaryCue { deck } if action.value == MidiValue::Release => {
                Some(Command::EndCuePreview { deck })
            }
            TemporaryCue { deck } => Some(Command::TriggerTemporaryCue { deck }),
            Cue { deck, index }
                if snapshot
                    .deck(deck)
                    .cues
                    .get(index as usize)
                    .is_some_and(|c| c.is_some()) =>
            {
                Some(Command::JumpCue { deck, index })
            }
            Sync { deck } if snapshot.deck(deck).synced => Some(Command::DisableSync { deck }),
            Sync { deck }
                if snapshot
                    .decks
                    .iter()
                    .all(|d| d.frames > 0 && d.grid_ready && !d.reverse) =>
            {
                Some(Command::Sync {
                    deck,
                    keylock: snapshot.deck(deck).keylock,
                })
            }
            _ => action.command(&snapshot),
        };
        let Some(command) = command else {
            return false;
        };
        // Serialize with the AutoMix commit, so a cancelled preparation cannot re-arm playback.
        let manual_takeover = matches!(
            command,
            Command::PlayPause { .. }
                | Command::JumpCue { .. }
                | Command::TriggerTemporaryCue { .. }
                | Command::BeginCuePreview { .. }
                | Command::SetBrake { on: true, .. }
        );
        let _commit = manual_takeover.then(|| {
            self.automix_commit
                .lock()
                .expect("controller manual takeover")
        });
        if manual_takeover {
            self.controller_automix_epoch.fetch_add(1, Ordering::AcqRel);
            self.controller_stop_automix.store(true, Ordering::Release);
            let _ = self.engine.dispatch(Command::StopAutomix);
        }
        if let Err(error) = self.engine.dispatch(command.clone()) {
            *self.midi_error.lock().expect("controller error") = Some(error.to_string());
        }
        let mut effects = self.controller_effects.lock().expect("controller effects");
        // A blocked UI needs at most three distinct cancellation masks; fader
        // traffic cannot crowd a later Jog/Sync cancellation out of the queue.
        let mask = crate::beat_sync::cancellation_mask(&command);
        if mask != 0
            && !effects
                .iter()
                .any(|previous| crate::beat_sync::cancellation_mask(previous) == mask)
        {
            effects.push(command);
        }
        drop(effects);
        if let Some(wake) = self
            .controller_wake
            .lock()
            .expect("controller wake")
            .clone()
        {
            wake();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_controllers::MidiAction;
    fn event(target: MidiTarget, value: MidiValue) -> ControlEvent {
        ControlEvent {
            device_id: "independent-test".into(),
            received_at: std::time::Instant::now(),
            action: MidiAction { target, value },
        }
    }
    #[test]
    fn performance_commands_dispatch_without_ui_and_library_load_preserves_order() {
        let (_temp, core, tracks) = crate::automix::tests::fixture();
        crate::analysis::load_manual(&core, DeckId::A, tracks[0], || true).unwrap();
        assert!(core.apply_controller_event(event(
            MidiTarget::Play { deck: DeckId::A },
            MidiValue::Press
        )));
        assert!(core.engine.snapshot().decks[0].playing);
        assert!(core.controller_stop_automix.load(Ordering::Acquire));
        assert!(
            core.apply_controller_event(event(MidiTarget::Master, MidiValue::Normalized(0.25)))
        );
        assert!((core.engine.snapshot().master - 0.25).abs() < 0.001);
        assert!(
            !core.apply_controller_event(event(MidiTarget::BrowseTracks, MidiValue::Relative(1)))
        );
        assert!(!core.apply_controller_event(event(
            MidiTarget::LoadSelected { deck: DeckId::A },
            MidiValue::Press
        )));
        assert!(!core.apply_controller_event(event(
            MidiTarget::Play { deck: DeckId::A },
            MidiValue::Press
        )));
        assert!(core.engine.snapshot().decks[0].playing); // queued load/play cannot toggle the previous buffer
        assert!(core.apply_controller_event(event(
            MidiTarget::JogTouch { deck: DeckId::A },
            MidiValue::Press
        )));
        assert!(core.apply_controller_event(event(
            MidiTarget::JogTouch { deck: DeckId::A },
            MidiValue::Release
        )));
    }
}
