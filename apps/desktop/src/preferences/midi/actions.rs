//! MIDI edit persistence and Learn lifecycle, separate from rendering.
use super::*;

impl Preferences {
    pub(in crate::preferences) fn start_binding_learn(&self) -> Result<(), String> {
        if self.controller_device.is_none() {
            return Err("Select a connected device before learning".into());
        }
        if self.job.is_some() {
            return Err("Wait for the current settings operation to finish.".into());
        }
        let address = self.binding.midi_address();
        let explicit =
            matches!(address, mixless_controllers::MidiAddress::Cc14 { .. }).then_some(address);
        self.hub()?
            .learn_address_for(
                self.binding.target.clone(),
                self.controller_device.clone(),
                explicit,
            )
            .map_err(|e| e.to_string())
    }

    pub(in crate::preferences) fn cancel_learn(&mut self) {
        self.learning = false;
        self.quick_learning = false;
        if let Some(hub) = self.core.midi.lock().expect("MIDI").as_ref() {
            hub.set_learn(false);
        }
    }

    pub(in crate::preferences) fn toggle_learn(&mut self) {
        if self.learning {
            self.cancel_learn();
            return;
        }
        self.midi_feedback = None;
        match self.start_binding_learn() {
            Ok(()) => self.learning = true,
            Err(e) => {
                self.learning = false;
                self.midi_feedback = Some((e, true));
            }
        }
    }

    pub(in crate::preferences) fn save_binding(&mut self) {
        if self.controller_device.is_none() && self.editing.is_none() {
            self.result(
                Err(
                    "Select a device before assigning; use Advanced mapping for offline presets"
                        .into(),
                ),
                "",
            );
            return;
        }
        let result = {
            let midi = self.core.midi.lock().expect("MIDI");
            midi.as_ref()
                .ok_or_else(|| "MIDI is unavailable".to_string())
                .and_then(|hub| {
                    if self.learning {
                        let message = hub
                            .learned()
                            .ok_or_else(|| "Waiting for a MIDI signal".to_string())?;
                        self.binding.device_id = Some(message.device_id);
                        self.binding.set_address(message.address);
                    }
                    if let Some(canonical) =
                        mixless_controllers::Binding::from_legacy(&self.binding)
                            .legacy(self.binding.device_id.clone())
                    {
                        self.binding = canonical;
                    }
                    if let Some(index) = self.editing {
                        hub.replace(index, self.binding.clone())
                            .map_err(|e| e.to_string())
                    } else {
                        hub.upsert(self.binding.clone()).map_err(|e| e.to_string())
                    }
                })
        };
        let saved = result.is_ok().then(|| self.binding.clone());
        if saved.is_some() {
            self.cancel_learn();
        }
        self.result(result, "MIDI mapping saved");
        self.poll();
        if let Some(saved) = saved {
            self.editing = self.bindings.iter().position(|binding| *binding == saved);
            self.binding = saved;
        }
    }
}
