//! Independent controller profiles and MIDI/HID runtime. No device I/O runs in the audio callback.

#[cfg(test)]
mod mapping_tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(test)]
use mixless_midi::midir;
pub use mixless_midi::{input_ports, output_ports, MidiPort};
#[cfg(test)]
use mixless_protocol::DeckId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MidiError {
    #[error("MIDI: {0}")]
    Init(String),
}

fn error(e: impl std::fmt::Display) -> MidiError {
    MidiError::Init(e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MidiConfig {
    pub enabled: bool,
    /// None connects all available inputs. Some(empty) connects none.
    pub input_ids: Option<Vec<String>>,
}

impl Default for MidiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            input_ids: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiSourceKind {
    Cc,
    Note,
}

mod catalogue;
mod control;
mod hid_backend;
mod targets;
pub use control::{MidiAction, MidiControlMode, MidiValue};
pub use targets::{ControlKind, MidiGroup, MidiTarget};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiBinding {
    /// Absent in legacy maps: match this address on any enabled input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    pub kind: MidiSourceKind,
    pub channel: u8,
    pub number: u8,
    pub target: MidiTarget,
    #[serde(default)]
    pub mode: MidiControlMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MidiMapFile {
    pub bindings: Vec<MidiBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MidiMessage {
    pub device_id: String,
    pub kind: MidiSourceKind,
    pub channel: u8,
    pub number: u8,
    pub value: u8,
}

#[derive(Default)]
struct InputState {
    learning: bool,
    learn_target: Option<MidiTarget>,
    learn_device: Option<String>,
    learned: Option<MidiMessage>,
    last: Option<MidiMessage>,
    queued: Vec<MidiAction>,
    held: Vec<(MidiMessage, MidiTarget)>,
}

impl InputState {
    fn start_learning(&mut self, target: Option<MidiTarget>) {
        self.learning = true;
        self.learn_target = target;
        self.learned = None;
        self.queued
            .retain(|action| action.value == MidiValue::Release);
        self.release_held();
    }

    fn release_held(&mut self) {
        for (_, target) in self.held.drain(..) {
            self.queued.push(MidiAction {
                target,
                value: MidiValue::Release,
            });
        }
    }
    fn receive(&mut self, bindings: &[MidiBinding], device_id: &str, msg: &[u8]) {
        let Some((kind, channel, number, value)) = parse_msg(msg) else {
            return;
        };
        let message = MidiMessage {
            device_id: device_id.into(),
            kind,
            channel,
            number,
            value,
        };
        self.last = Some(message.clone());
        if self.learning {
            let compatible = match self.learn_target.as_ref().map(MidiTarget::kind) {
                Some(ControlKind::Continuous) => kind == MidiSourceKind::Cc,
                Some(ControlKind::Encoder) => {
                    kind == MidiSourceKind::Cc && !matches!(value, 0 | 64)
                }
                Some(ControlKind::Button | ControlKind::Momentary) => value > 0,
                None => !(kind == MidiSourceKind::Note && value == 0),
            };
            if self.learned.is_none() && compatible {
                self.learned = Some(message);
            }
            return;
        }
        if let Some(cmd) = resolve(bindings, device_id, msg) {
            if cmd.target.kind() == ControlKind::Momentary {
                let same = |(held, _): &(MidiMessage, MidiTarget)| {
                    held.device_id == device_id
                        && held.kind == kind
                        && held.channel == channel
                        && held.number == number
                };
                if cmd.value == MidiValue::Press {
                    if self.held.iter().any(same) {
                        return;
                    }
                    if self.queued.len() >= 1024 {
                        return;
                    }
                    self.held.push((message, cmd.target.clone()));
                } else {
                    self.held.retain(|entry| !same(entry));
                    if self.queued.len() >= 1024 {
                        self.queued.remove(0);
                    }
                }
            }
            // Keep pathological controllers from growing the queue without bound.
            if self.queued.len() < 1024 {
                self.queued.push(cmd);
            }
        }
    }
}

mod manager;
mod profile;
mod router;
mod runtime;
pub use manager::*;
pub use profile::*;
pub use runtime::{ConnectionState, ControllerDevice, ReceivedPacket};

fn same_source(a: &MidiBinding, b: &MidiBinding) -> bool {
    a.device_id == b.device_id && a.kind == b.kind && a.channel == b.channel && a.number == b.number
}

fn validate(binding: &MidiBinding) -> Result<(), MidiError> {
    if binding.channel > 15 || binding.number > 127 {
        return Err(error(
            "channel must be 1-16 and controller / note must be 0-127",
        ));
    }
    if !MidiTarget::all().contains(&binding.target) {
        return Err(error("unknown MIDI control or out-of-range cue / FX slot"));
    }
    let relative = binding.mode.relative(&binding.target);
    if binding.target.kind() == ControlKind::Encoder && !relative {
        return Err(error("jog and browse encoders require a relative CC mode"));
    }
    if relative
        && (binding.kind != MidiSourceKind::Cc
            || matches!(
                binding.target.kind(),
                ControlKind::Button | ControlKind::Momentary
            ))
    {
        return Err(error("relative mode requires a CC knob or encoder"));
    }
    Ok(())
}

fn resolve(bindings: &[MidiBinding], device_id: &str, msg: &[u8]) -> Option<MidiAction> {
    let (kind, ch, num, val) = parse_msg(msg)?;
    let binding = bindings
        .iter()
        .filter(|b| b.kind == kind && b.channel == ch && b.number == num)
        .find(|b| b.device_id.as_deref() == Some(device_id))
        .or_else(|| {
            bindings.iter().find(|b| {
                b.kind == kind && b.channel == ch && b.number == num && b.device_id.is_none()
            })
        })?;
    let value = match binding.target.kind() {
        ControlKind::Momentary => {
            if val == 0 {
                MidiValue::Release
            } else {
                MidiValue::Press
            }
        }
        ControlKind::Button => {
            if val == 0 {
                return None;
            }
            MidiValue::Press
        }
        _ if binding.mode.relative(&binding.target) => {
            let delta = binding.mode.delta(val);
            if delta == 0 {
                return None;
            }
            MidiValue::Relative(delta)
        }
        _ => {
            if kind == MidiSourceKind::Note && val == 0 {
                return None;
            }
            if binding.mode == MidiControlMode::AbsoluteInverted {
                MidiValue::Absolute(127 - val)
            } else {
                MidiValue::Absolute(val)
            }
        }
    };
    Some(MidiAction {
        target: binding.target.clone(),
        value,
    })
}

fn parse_msg(msg: &[u8]) -> Option<(MidiSourceKind, u8, u8, u8)> {
    if msg.len() != 3 || msg[1] > 127 || msg[2] > 127 {
        return None;
    }
    let ch = msg[0] & 0x0f;
    match msg[0] & 0xf0 {
        0x90 => Some((MidiSourceKind::Note, ch, msg[1], msg[2])),
        0x80 => Some((MidiSourceKind::Note, ch, msg[1], 0)),
        0xb0 => Some((MidiSourceKind::Cc, ch, msg[1], msg[2])),
        _ => None,
    }
}

fn read_map(path: &Path) -> Result<Vec<MidiBinding>, MidiError> {
    let map: MidiMapFile =
        serde_json::from_slice(&fs::read(path).map_err(error)?).map_err(error)?;
    for (index, binding) in map.bindings.iter().enumerate() {
        validate(binding)?;
        if map.bindings[..index]
            .iter()
            .any(|b| same_source(b, binding))
        {
            return Err(error("duplicate MIDI source in mapping file"));
        }
    }
    Ok(map.bindings)
}

fn save_map(path: &Path, bindings: &[MidiBinding]) -> Result<(), MidiError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(error)?;
    }
    let bytes = serde_json::to_vec_pretty(&MidiMapFile {
        bindings: bindings.to_vec(),
    })
    .map_err(error)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(error)?;
    fs::rename(&temp, path).map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> MidiBinding {
        MidiBinding {
            device_id: None,
            kind: MidiSourceKind::Note,
            channel: 0,
            number: 60,
            target: MidiTarget::Play { deck: DeckId::A },
            mode: MidiControlMode::Auto,
        }
    }

    #[test]
    fn releases_and_malformed_messages_never_trigger_buttons() {
        let map = [binding()];
        assert!(matches!(
            resolve(&map, "one", &[0x90, 60, 127]),
            Some(MidiAction {
                target: MidiTarget::Play { .. },
                ..
            })
        ));
        for msg in [
            &[0x90, 60, 0][..],
            &[0x80, 60, 127],
            &[0x90, 60],
            &[0x90, 60, 255],
        ] {
            assert!(resolve(&map, "one", msg).is_none());
        }
        let map = [MidiBinding {
            kind: MidiSourceKind::Cc,
            ..binding()
        }];
        assert!(resolve(&map, "one", &[0xb0, 60, 0]).is_none());
    }

    #[test]
    fn learning_captures_first_press_and_suppresses_commands() {
        let mut input = InputState {
            learning: true,
            ..Default::default()
        };
        input.receive(&[binding()], "one", &[0x80, 60, 0]);
        assert!(input.learned.is_none());
        input.receive(&[binding()], "one", &[0x90, 60, 127]);
        input.receive(&[binding()], "two", &[0x90, 61, 127]);
        assert_eq!(input.learned.as_ref().unwrap().device_id, "one");
        assert_eq!(input.learned.as_ref().unwrap().number, 60);
        assert!(input.queued.is_empty());
    }

    #[test]
    fn port_specific_mapping_takes_priority_over_legacy_wildcard() {
        let map = [
            binding(),
            MidiBinding {
                device_id: Some("two".into()),
                target: MidiTarget::Play { deck: DeckId::B },
                ..binding()
            },
        ];
        assert!(matches!(
            resolve(&map, "one", &[0x90, 60, 1]),
            Some(MidiAction {
                target: MidiTarget::Play { deck: DeckId::A },
                ..
            })
        ));
        assert!(matches!(
            resolve(&map, "two", &[0x90, 60, 1]),
            Some(MidiAction {
                target: MidiTarget::Play { deck: DeckId::B },
                ..
            })
        ));
        assert!(validate(&MidiBinding {
            channel: 16,
            ..binding()
        })
        .is_err());
        assert!(validate(&MidiBinding {
            target: MidiTarget::Cue {
                deck: DeckId::A,
                index: 8
            },
            ..binding()
        })
        .is_err());
    }

    #[test]
    fn mapping_edits_persist_and_invalid_import_keeps_current_map() {
        let dir = std::env::temp_dir().join(format!("mixless-midi-map-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("midi.json");
        let hub = MidiHub::start_with_config(
            path.clone(),
            MidiConfig {
                enabled: false,
                input_ids: None,
            },
        )
        .unwrap();
        hub.upsert(binding()).unwrap();
        let replacement = MidiBinding {
            number: 61,
            ..binding()
        };
        hub.replace(0, replacement.clone()).unwrap();
        assert_eq!(
            profile::PresetStore::open(&path).unwrap().flattened(),
            vec![replacement.clone()]
        );
        let export = dir.join("export.json");
        hub.export(&export).unwrap();
        hub.remove(0).unwrap();
        assert!(hub.bindings().is_empty());
        hub.import(&export).unwrap();
        assert_eq!(hub.bindings(), vec![replacement]);
        fs::write(&export, br#"{"bindings":[{"kind":"note","channel":20,"number":60,"target":{"type":"master"}}]}"#).unwrap();
        assert!(hub.import(&export).is_err());
        assert_eq!(hub.bindings().len(), 1);
        fs::remove_file(&hub.path).unwrap();
        fs::create_dir(&hub.path).unwrap();
        assert!(hub.remove(0).is_err());
        assert_eq!(hub.bindings().len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "opens a temporary CoreMIDI virtual input"]
    fn virtual_controller_learn_and_mapping_smoke() {
        use midir::os::unix::VirtualOutput;
        use std::time::{Duration, Instant};
        let name = format!("Mixless MIDI test {}", std::process::id());
        let mut output = midir::MidiOutput::new(&name)
            .unwrap()
            .create_virtual(&name)
            .unwrap();
        let port = input_ports()
            .unwrap()
            .into_iter()
            .find(|p| p.name == name)
            .unwrap();
        let path =
            std::env::temp_dir().join(format!("mixless-midi-virtual-{}.json", std::process::id()));
        let hub = MidiHub::start_with_config(
            path.clone(),
            MidiConfig {
                enabled: true,
                input_ids: Some(vec![port.id.clone()]),
            },
        )
        .unwrap();
        hub.learn_target(MidiTarget::Master).unwrap();
        output.send(&[0xb0, 7, 100]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while hub.learned().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(hub.drain().is_empty());
        hub.bind(MidiTarget::Master).unwrap();
        assert_eq!(
            hub.bindings()[0].device_id.as_deref(),
            Some(port.id.as_str())
        );
        output.send(&[0xb0, 7, 127]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut commands = Vec::new();
        while commands.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            commands = hub.drain();
        }
        assert!(matches!(
            commands.as_slice(),
            [MidiAction {
                target: MidiTarget::Master,
                value: MidiValue::Absolute(127)
            }]
        ));
        hub.configure(MidiConfig {
            enabled: false,
            input_ids: None,
        })
        .unwrap();
        fs::remove_file(&hub.path).unwrap();
    }
}

#[cfg(test)]
mod profile_tests;
