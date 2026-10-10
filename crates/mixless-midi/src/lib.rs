//! MIDI transport and MIDI 1.0 message decoding. No application control semantics.
pub use midir;
use midir::{MidiInput, MidiOutput};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiPort {
    pub id: String,
    pub name: String,
}

pub fn input_ports() -> Result<Vec<MidiPort>, String> {
    let midi = MidiInput::new("mixless-probe").map_err(|e| e.to_string())?;
    midi.ports()
        .iter()
        .map(|p| {
            Ok(MidiPort {
                id: p.id(),
                name: midi.port_name(p).map_err(|e| e.to_string())?,
            })
        })
        .collect()
}
pub fn output_ports() -> Result<Vec<MidiPort>, String> {
    let midi = MidiOutput::new("mixless-output-probe").map_err(|e| e.to_string())?;
    midi.ports()
        .iter()
        .map(|p| {
            Ok(MidiPort {
                id: p.id(),
                name: midi.port_name(p).map_err(|e| e.to_string())?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Address {
    Note {
        channel: u8,
        number: u8,
    },
    Cc {
        channel: u8,
        number: u8,
    },
    PitchBend {
        channel: u8,
    },
    Cc14 {
        channel: u8,
        msb: u8,
        lsb: u8,
    },
    Rpn {
        channel: u8,
        parameter: u16,
    },
    Nrpn {
        channel: u8,
        parameter: u16,
    },
    Sysex {
        prefix: Vec<u8>,
        offset: usize,
        bytes: u8,
    },
}
impl Address {
    pub fn valid(&self) -> bool {
        match self {
            Self::Note { channel, number } | Self::Cc { channel, number } => {
                *channel < 16 && *number < 128
            }
            Self::PitchBend { channel } => *channel < 16,
            Self::Cc14 { channel, msb, lsb } => {
                *channel < 16 && *msb < 128 && *lsb < 128 && msb != lsb
            }
            Self::Rpn { channel, parameter } | Self::Nrpn { channel, parameter } => {
                *channel < 16 && *parameter < 16384
            }
            Self::Sysex {
                prefix,
                offset,
                bytes,
            } => {
                prefix.first() == Some(&0xf0)
                    && prefix.len() <= 256
                    && prefix[1..].iter().all(|b| *b < 128)
                    && *offset >= prefix.len()
                    && *offset < 4096
                    && (1..=4).contains(bytes)
            }
        }
    }
}
#[derive(Clone, Default)]
struct Parameter {
    number: [Option<u8>; 2],
    data: [Option<u8>; 2],
    nrpn: bool,
}
#[derive(Default)]
pub struct Decoder {
    cc: HashMap<(u8, u8), (u8, u64)>,
    parameters: HashMap<u8, Parameter>,
    pairs: HashMap<(u8, u8, u8), (u64, u64)>,
    sequence: u64,
    last: Vec<u8>,
}
impl Decoder {
    /// Call once per received message, then evaluate each binding with `value`.
    pub fn receive(&mut self, message: &[u8]) {
        self.sequence = self.sequence.wrapping_add(1);
        self.last.clear();
        if message.len() > 4096 {
            return;
        }
        self.last.extend_from_slice(message);
        if let [status, number, value] = message {
            if status & 0xf0 != 0xb0 || *number > 127 || *value > 127 {
                return;
            }
            let ch = status & 15;
            self.cc.insert((ch, *number), (*value, self.sequence));
            let p = self.parameters.entry(ch).or_default();
            match *number {
                99 | 101 => {
                    let nrpn = *number == 99;
                    if p.nrpn != nrpn {
                        *p = Parameter::default();
                    }
                    p.nrpn = nrpn;
                    p.number[0] = Some(*value);
                    p.data = [None; 2];
                }
                98 | 100 => {
                    let nrpn = *number == 98;
                    if p.nrpn != nrpn {
                        *p = Parameter::default();
                    }
                    p.nrpn = nrpn;
                    p.number[1] = Some(*value);
                    p.data = [None; 2];
                }
                6 => p.data[0] = Some(*value),
                38 => p.data[1] = Some(*value),
                _ => {}
            }
        }
    }
    pub fn value(&mut self, address: &Address) -> Option<(u32, u32)> {
        if !address.valid() {
            return None;
        }
        let m = &self.last;
        if let Address::Sysex {
            prefix,
            offset,
            bytes,
        } = address
        {
            if !m.starts_with(prefix) || m.last() != Some(&0xf7) {
                return None;
            }
            let end = offset.checked_add(*bytes as usize)?;
            if end >= m.len() {
                return None;
            }
            let data = m.get(*offset..end)?;
            if data.iter().any(|b| *b > 127) {
                return None;
            }
            return Some((
                data.iter().fold(0u32, |v, b| (v << 7) | *b as u32),
                (1u32 << (*bytes as u32 * 7)) - 1,
            ));
        }
        let [s, n, v] = m.as_slice() else {
            return None;
        };
        if *n > 127 || *v > 127 {
            return None;
        }
        let (ch, kind) = (s & 15, s & 0xf0);
        match address {
            Address::Note { channel, number }
                if ch == *channel && n == number && matches!(kind, 0x80 | 0x90) =>
            {
                Some((if kind == 0x80 { 0 } else { *v as u32 }, 127))
            }
            Address::Cc { channel, number } if kind == 0xb0 && ch == *channel && n == number => {
                Some((*v as u32, 127))
            }
            Address::PitchBend { channel } if kind == 0xe0 && ch == *channel => {
                Some((((*v as u32) << 7) | *n as u32, 16383))
            }
            Address::Cc14 { channel, msb, lsb }
                if kind == 0xb0 && ch == *channel && (n == msb || n == lsb) =>
            {
                let &(hi, a) = self.cc.get(&(ch, *msb))?;
                let &(lo, b) = self.cc.get(&(ch, *lsb))?;
                // Do not combine halves from a distant control gesture.
                let consumed = self.pairs.entry((ch, *msb, *lsb)).or_default();
                if consumed.1 == self.sequence {
                    return Some((((hi as u32) << 7) | lo as u32, 16383));
                }
                if a.min(b) <= consumed.0 || a.abs_diff(b) > 32 {
                    return None;
                }
                *consumed = (a.max(b), self.sequence);
                Some((((hi as u32) << 7) | lo as u32, 16383))
            }
            Address::Rpn { channel, parameter } | Address::Nrpn { channel, parameter }
                if kind == 0xb0 && ch == *channel && matches!(*n, 6 | 38) =>
            {
                let p = self.parameters.get(&ch)?;
                if p.nrpn != matches!(address, Address::Nrpn { .. }) {
                    return None;
                }
                let number = ((p.number[0]? as u16) << 7) | p.number[1]? as u16;
                if number == 16383 || number != *parameter {
                    return None;
                }
                Some((
                    ((p.data[0]? as u32) << 7) | p.data[1].unwrap_or(0) as u32,
                    16383,
                ))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairs_emit_once_without_stale_halves_in_either_order() {
        let mut d = Decoder::default();
        let a = Address::Cc14 {
            channel: 0,
            msb: 3,
            lsb: 35,
        };
        for (m, expected) in [
            ([0xb0, 3, 10], None),
            ([0xb0, 35, 11], Some((1291, 16383))),
            ([0xb0, 3, 12], None),
            ([0xb0, 35, 13], Some((1549, 16383))),
            ([0xb0, 35, 14], None),
            ([0xb0, 3, 15], Some((1934, 16383))),
        ] {
            d.receive(&m);
            assert_eq!(d.value(&a), expected);
            assert_eq!(d.value(&a), expected); // two layered bindings see the same completed value
        }
    }
    #[test]
    fn sysex_boundaries_null_selection_and_malformed_packets() {
        let mut d = Decoder::default();
        let a = Address::Sysex {
            prefix: vec![0xf0, 0x7d, 1],
            offset: 3,
            bytes: 2,
        };
        d.receive(&[0xf0, 0x7d, 1, 2, 3, 0xf7]);
        assert_eq!(d.value(&a), Some((259, 16383)));
        for m in [
            vec![0xf0, 0x7d, 1, 2, 0xf7],
            vec![0xf0, 0x7d, 1, 0x80, 3, 0xf7],
            vec![0xf0, 0x7d, 1, 2, 3],
        ] {
            d.receive(&m);
            assert_eq!(d.value(&a), None);
        }
        for m in [[0xb0, 101, 127], [0xb0, 100, 127], [0xb0, 6, 20]] {
            d.receive(&m);
        }
        assert_eq!(
            d.value(&Address::Rpn {
                channel: 0,
                parameter: 16383
            }),
            None
        );
        d.receive(&[0xe0, 128, 64]);
        assert_eq!(d.value(&Address::PitchBend { channel: 0 }), None);
    }
    #[test]
    fn note_release_and_pitch_precision() {
        let mut d = Decoder::default();
        d.receive(&[0x82, 7, 90]);
        assert_eq!(
            d.value(&Address::Note {
                channel: 2,
                number: 7
            }),
            Some((0, 127))
        );
        d.receive(&[0xe3, 1, 64]);
        assert_eq!(
            d.value(&Address::PitchBend { channel: 3 }),
            Some((8193, 16383))
        );
    }
    #[test]
    fn paired_cc_and_parameter_selection_are_channel_local() {
        let mut d = Decoder::default();
        let a = Address::Cc14 {
            channel: 0,
            msb: 1,
            lsb: 33,
        };
        d.receive(&[0xb0, 1, 20]);
        assert_eq!(d.value(&a), None);
        d.receive(&[0xb1, 33, 10]);
        assert_eq!(d.value(&a), None);
        d.receive(&[0xb0, 33, 7]);
        assert_eq!(d.value(&a), Some((2567, 16383)));
        for m in [[0xb0, 99, 1], [0xb0, 98, 2], [0xb0, 6, 40], [0xb0, 38, 5]] {
            d.receive(&m);
        }
        assert_eq!(
            d.value(&Address::Nrpn {
                channel: 0,
                parameter: 130
            }),
            Some((5125, 16383))
        );
        assert_eq!(
            d.value(&Address::Rpn {
                channel: 0,
                parameter: 130
            }),
            None
        );
    }
}
