//! Learn exact addresses; paired CCs are learned only when explicitly selected.
use crate::*;

pub(super) fn default_sensitivity() -> f64 {
    1.
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnedInput {
    pub device_id: String,
    pub address: MidiAddress,
    pub value: u32,
}

impl LearnedInput {
    pub(super) fn from_packet(device: &str, bytes: &[u8]) -> Option<Self> {
        let [status, number, value] = bytes else {
            return None;
        };
        if *number > 127 || *value > 127 {
            return None;
        }
        let (address, value) = match status & 0xf0 {
            0x80 | 0x90 => (
                MidiAddress::Note {
                    channel: status & 15,
                    number: *number,
                },
                if status & 0xf0 == 0x80 {
                    0
                } else {
                    *value as u32
                },
            ),
            0xb0 => (
                MidiAddress::Cc {
                    channel: status & 15,
                    number: *number,
                },
                *value as u32,
            ),
            0xe0 => (
                MidiAddress::PitchBend {
                    channel: status & 15,
                },
                ((*value as u32) << 7) | *number as u32,
            ),
            _ => return None,
        };
        Some(Self {
            device_id: device.into(),
            address,
            value,
        })
    }
}

impl MidiBinding {
    pub fn midi_address(&self) -> MidiAddress {
        self.address.clone().unwrap_or_else(|| match self.kind {
            MidiSourceKind::Cc => MidiAddress::Cc {
                channel: self.channel,
                number: self.number,
            },
            MidiSourceKind::Note => MidiAddress::Note {
                channel: self.channel,
                number: self.number,
            },
        })
    }
    pub fn update_channel_number(&mut self, channel: u8, number: u8) {
        let address = match self.midi_address() {
            MidiAddress::Note { .. } => MidiAddress::Note { channel, number },
            MidiAddress::Cc14 { lsb, .. } => MidiAddress::Cc14 {
                channel,
                msb: number,
                lsb,
            },
            MidiAddress::PitchBend { .. } => MidiAddress::PitchBend { channel },
            _ => MidiAddress::Cc { channel, number },
        };
        self.set_address(address);
    }
    pub fn set_address(&mut self, address: MidiAddress) {
        let (channel, number, kind) = match address {
            MidiAddress::Note { channel, number } => (channel, number, MidiSourceKind::Note),
            MidiAddress::Cc { channel, number } => (channel, number, MidiSourceKind::Cc),
            MidiAddress::Cc14 { channel, msb, .. } => (channel, msb, MidiSourceKind::Cc),
            MidiAddress::PitchBend { channel } => (channel, 0, MidiSourceKind::Cc),
            _ => (0, 0, MidiSourceKind::Cc),
        };
        self.channel = channel;
        self.number = number;
        self.kind = kind;
        self.address = match address {
            MidiAddress::Note { .. } | MidiAddress::Cc { .. } => None,
            _ => Some(address),
        };
    }
}

impl InputState {
    pub(super) fn forget_device(&mut self, device: &str) {
        self.learn_decoders.remove(device);
        if self
            .learned
            .as_ref()
            .is_some_and(|input| input.device_id == device)
        {
            self.learned = None;
        }
    }

    pub(super) fn receive_learn(&mut self, device: &str, bytes: &[u8]) {
        if self.learn_device.as_deref().is_some_and(|id| id != device) || self.learned.is_some() {
            return;
        }
        let address = if let Some(address) = &self.learn_address {
            address.clone()
        } else {
            match bytes {
                [status, number, value] if *number < 128 && *value < 128 => match status & 0xf0 {
                    0x90 | 0x80 => MidiAddress::Note {
                        channel: status & 15,
                        number: *number,
                    },
                    0xb0 => MidiAddress::Cc {
                        channel: status & 15,
                        number: *number,
                    },
                    0xe0 => MidiAddress::PitchBend {
                        channel: status & 15,
                    },
                    _ => return,
                },
                _ => return,
            }
        };
        let decoder = self.learn_decoders.entry(device.into()).or_default();
        decoder.receive(bytes);
        let Some((value, _)) = decoder.value(&address) else {
            return;
        };
        let compatible = match self.learn_target.as_ref().map(MidiTarget::kind) {
            Some(ControlKind::Continuous) => !matches!(address, MidiAddress::Note { .. }),
            Some(ControlKind::Encoder) => {
                matches!(address, MidiAddress::Cc { .. }) && !matches!(value, 0 | 64)
            }
            Some(ControlKind::Button | ControlKind::Momentary) => {
                !matches!(
                    address,
                    MidiAddress::PitchBend { .. } | MidiAddress::Cc14 { .. }
                ) && value > 0
            }
            None => !matches!(address, MidiAddress::Note { .. }) || value > 0,
        };
        if compatible {
            self.learned = Some(LearnedInput {
                device_id: device.into(),
                address,
                value,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn learns_pitch_bend_and_only_explicit_pairs_without_cross_device_pairing() {
        let mut input = InputState::default();
        input.start_learning(Some(MidiTarget::CueMix));
        input.receive(&[], "one", &[0xe3, 1, 64]);
        assert_eq!(
            input.learned.as_ref().unwrap().address,
            MidiAddress::PitchBend { channel: 3 }
        );
        assert_eq!(input.learned.as_ref().unwrap().value, 8193);
        assert_eq!(input.last_input.as_ref().unwrap().value, 8193);
        assert!(input.queued.is_empty());
        input.start_learning(Some(MidiTarget::Master));
        input.receive(&[], "one", &[0xb0, 1, 100]);
        assert_eq!(input.last.as_ref().unwrap().value, 100);
        assert_eq!(
            input.learned.as_ref().unwrap().address,
            MidiAddress::Cc {
                channel: 0,
                number: 1
            }
        );
        input.start_learning(Some(MidiTarget::Master));
        input.learn_address = Some(MidiAddress::Cc14 {
            channel: 0,
            msb: 1,
            lsb: 33,
        });
        input.receive(&[], "one", &[0xb0, 1, 100]);
        input.receive(&[], "two", &[0xb0, 33, 7]);
        assert!(input.learned.is_none());
        input.receive(&[], "one", &[0xb0, 33, 7]);
        assert_eq!(input.last.as_ref().unwrap().number, 33);
        assert_eq!(input.learned.unwrap().value, 12807);
    }
}
