//! MIDI input and learning stay outside the realtime audio thread.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use midir::{Ignore, MidiInput, MidiInputConnection};
use mixless_protocol::{Command, DeckId, EqBand};
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

#[derive(Debug, Clone)]
pub struct MidiPort {
    pub id: String,
    pub name: String,
}

pub fn input_ports() -> Result<Vec<MidiPort>, MidiError> {
    let midi = MidiInput::new("mixless-probe").map_err(error)?;
    midi.ports()
        .iter()
        .map(|port| {
            Ok(MidiPort {
                id: port.id(),
                name: midi.port_name(port).map_err(error)?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiSourceKind {
    Cc,
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MidiTarget {
    Xfader,
    Master,
    CueGain,
    Fader { deck: DeckId },
    Gain { deck: DeckId },
    Eq { deck: DeckId, band: EqBand },
    Filter { deck: DeckId },
    Resonance { deck: DeckId },
    Tempo { deck: DeckId },
    Pitch { deck: DeckId },
    Play { deck: DeckId },
    Cue { deck: DeckId, index: u8 },
    Jog { deck: DeckId },
    Sync { deck: DeckId },
}

impl MidiTarget {
    pub fn label(&self) -> String {
        match self {
            Self::Xfader => "Crossfader".into(),
            Self::Master => "Master volume".into(),
            Self::CueGain => "Headphone volume".into(),
            Self::Fader { deck } => format!("Deck {deck:?} / Channel fader"),
            Self::Gain { deck } => format!("Deck {deck:?} / Trim"),
            Self::Eq { deck, band } => format!("Deck {deck:?} / EQ {band:?}"),
            Self::Filter { deck } => format!("Deck {deck:?} / Filter"),
            Self::Resonance { deck } => format!("Deck {deck:?} / Resonance"),
            Self::Tempo { deck } => format!("Deck {deck:?} / Tempo"),
            Self::Pitch { deck } => format!("Deck {deck:?} / Key"),
            Self::Play { deck } => format!("Deck {deck:?} / Play / pause"),
            Self::Cue { deck, index } => format!("Deck {deck:?} / Hot cue {}", index + 1),
            Self::Jog { deck } => format!("Deck {deck:?} / Jog (relative CC)"),
            Self::Sync { deck } => format!("Deck {deck:?} / Sync"),
        }
    }

    pub fn all() -> Vec<Self> {
        let mut targets = vec![Self::Xfader, Self::Master, Self::CueGain];
        for deck in [DeckId::A, DeckId::B] {
            targets.extend([
                Self::Play { deck },
                Self::Sync { deck },
                Self::Fader { deck },
                Self::Gain { deck },
                Self::Filter { deck },
                Self::Resonance { deck },
                Self::Tempo { deck },
                Self::Pitch { deck },
                Self::Jog { deck },
            ]);
            targets.extend(
                [EqBand::Low, EqBand::Mid, EqBand::High].map(|band| Self::Eq { deck, band }),
            );
            targets.extend((0..8).map(|index| Self::Cue { deck, index }));
        }
        targets
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiBinding {
    /// Absent in legacy maps: match this address on any enabled input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    pub kind: MidiSourceKind,
    pub channel: u8,
    pub number: u8,
    pub target: MidiTarget,
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
    learned: Option<MidiMessage>,
    last: Option<MidiMessage>,
    queued: Vec<Command>,
}

impl InputState {
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
            if self.learned.is_none() && !(kind == MidiSourceKind::Note && value == 0) {
                self.learned = Some(message);
            }
            return;
        }
        if let Some(cmd) = resolve(bindings, device_id, msg) {
            // Keep pathological controllers from growing the queue without bound.
            if self.queued.len() < 1024 {
                self.queued.push(cmd);
            }
        }
    }
}

pub struct MidiHub {
    bindings: Arc<Mutex<Vec<MidiBinding>>>,
    input: Arc<Mutex<InputState>>,
    path: PathBuf,
    config: Mutex<MidiConfig>,
    conns: Mutex<Vec<MidiInputConnection<()>>>,
}

impl MidiHub {
    pub fn start(map_path: PathBuf) -> Result<Self, MidiError> {
        Self::start_with_config(map_path, MidiConfig::default())
    }

    pub fn start_with_config(path: PathBuf, config: MidiConfig) -> Result<Self, MidiError> {
        let bindings = if path.exists() {
            read_map(&path)?
        } else {
            Vec::new()
        };
        let hub = Self {
            bindings: Arc::new(Mutex::new(bindings)),
            input: Arc::new(Mutex::new(InputState::default())),
            path,
            config: Mutex::new(MidiConfig::default()),
            conns: Mutex::new(Vec::new()),
        };
        hub.configure(config)?;
        Ok(hub)
    }

    pub fn reconnect(&self) -> Result<(), MidiError> {
        let config = self.config.lock().expect("MIDI config").clone();
        self.configure(config)
    }

    pub fn configure(&self, config: MidiConfig) -> Result<(), MidiError> {
        let mut conns = self.conns.lock().expect("MIDI connections");
        let mut next = Vec::new();
        // New connections cannot dispatch until they have replaced the old set.
        let active = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if config.enabled {
            let probe = MidiInput::new("mixless-probe").map_err(error)?;
            for port in probe.ports() {
                let id = port.id();
                if config
                    .input_ids
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(&id))
                {
                    continue;
                }
                let mut midi = MidiInput::new("mixless").map_err(error)?;
                midi.ignore(Ignore::All);
                let input = self.input.clone();
                let bindings = self.bindings.clone();
                let active = active.clone();
                let name = probe.port_name(&port).map_err(error)?;
                next.push(
                    midi.connect(
                        &port,
                        &format!("mixless-{name}"),
                        move |_, msg, _| {
                            if !active.load(std::sync::atomic::Ordering::Acquire) {
                                return;
                            }
                            if let (Ok(bindings), Ok(mut input)) = (bindings.lock(), input.lock()) {
                                input.receive(&bindings, &id, msg);
                            }
                        },
                        (),
                    )
                    .map_err(error)?,
                );
            }
        }
        conns.clear();
        self.set_learn(false);
        self.input.lock().expect("MIDI input").queued.clear();
        *conns = next;
        active.store(true, std::sync::atomic::Ordering::Release);
        *self.config.lock().expect("MIDI config") = config;
        Ok(())
    }

    pub fn set_learn(&self, on: bool) {
        let mut input = self.input.lock().expect("MIDI input");
        input.learning = on;
        input.learned = None;
        input.queued.clear();
    }

    pub fn learned(&self) -> Option<MidiMessage> {
        self.input.lock().expect("MIDI input").learned.clone()
    }
    pub fn last_message(&self) -> Option<MidiMessage> {
        self.input.lock().expect("MIDI input").last.clone()
    }

    pub fn last_seen(&self) -> Option<(MidiSourceKind, u8, u8, u8)> {
        self.last_message()
            .map(|m| (m.kind, m.channel, m.number, m.value))
    }

    pub fn bind(&self, target: MidiTarget) -> Result<(), MidiError> {
        let message = self
            .learned()
            .ok_or_else(|| error("no MIDI received while learning"))?;
        self.upsert(MidiBinding {
            device_id: Some(message.device_id),
            kind: message.kind,
            channel: message.channel,
            number: message.number,
            target,
        })?;
        self.set_learn(false);
        Ok(())
    }

    pub fn upsert(&self, binding: MidiBinding) -> Result<(), MidiError> {
        validate(&binding)?;
        self.edit_map(|bindings| {
            bindings.retain(|b| !same_source(b, &binding));
            bindings.push(binding);
        })
    }

    pub fn remove(&self, index: usize) -> Result<(), MidiError> {
        self.edit_map(|bindings| {
            if index < bindings.len() {
                bindings.remove(index);
            }
        })
    }

    pub fn replace(&self, index: usize, binding: MidiBinding) -> Result<(), MidiError> {
        validate(&binding)?;
        self.edit_map(|bindings| {
            if index < bindings.len() {
                bindings.remove(index);
            }
            bindings.retain(|b| !same_source(b, &binding));
            bindings.push(binding);
        })
    }

    fn edit_map(&self, edit: impl FnOnce(&mut Vec<MidiBinding>)) -> Result<(), MidiError> {
        let mut current = self.bindings.lock().expect("MIDI map");
        let mut next = current.clone();
        edit(&mut next);
        save_map(&self.path, &next)?;
        *current = next;
        Ok(())
    }

    pub fn import(&self, path: &Path) -> Result<(), MidiError> {
        let bindings = read_map(path)?;
        self.edit_map(|current| *current = bindings)
    }

    pub fn export(&self, path: &Path) -> Result<(), MidiError> {
        save_map(path, &self.bindings())
    }
    pub fn bindings(&self) -> Vec<MidiBinding> {
        self.bindings.lock().expect("MIDI map").clone()
    }
    pub fn drain(&self) -> Vec<Command> {
        self.input
            .lock()
            .expect("MIDI input")
            .queued
            .drain(..)
            .collect()
    }
    pub fn map_message(&self, msg: &[u8]) -> Option<Command> {
        resolve(&self.bindings.lock().ok()?, "", msg)
    }
    pub fn poll_and_apply<F: FnMut(Command)>(&self, raw: &[u8], mut apply: F) {
        if let Some(cmd) = self.map_message(raw) {
            apply(cmd);
        }
    }
}

fn same_source(a: &MidiBinding, b: &MidiBinding) -> bool {
    a.device_id == b.device_id && a.kind == b.kind && a.channel == b.channel && a.number == b.number
}

fn validate(binding: &MidiBinding) -> Result<(), MidiError> {
    if binding.channel > 15 || binding.number > 127 {
        return Err(error(
            "channel must be 1-16 and controller / note must be 0-127",
        ));
    }
    if matches!(binding.target, MidiTarget::Cue { index: 8.., .. }) {
        return Err(error("hot cue must be 1-8"));
    }
    if matches!(binding.target, MidiTarget::Jog { .. }) && binding.kind != MidiSourceKind::Cc {
        return Err(error("jog requires a relative CC controller"));
    }
    Ok(())
}

fn resolve(bindings: &[MidiBinding], device_id: &str, msg: &[u8]) -> Option<Command> {
    let (kind, ch, num, val) = parse_msg(msg)?;
    if kind == MidiSourceKind::Note && val == 0 {
        return None;
    }
    let binding = bindings
        .iter()
        .filter(|b| b.kind == kind && b.channel == ch && b.number == num)
        .find(|b| b.device_id.as_deref() == Some(device_id))
        .or_else(|| {
            bindings.iter().find(|b| {
                b.kind == kind && b.channel == ch && b.number == num && b.device_id.is_none()
            })
        })?;
    if matches!(
        binding.target,
        MidiTarget::Play { .. } | MidiTarget::Cue { .. } | MidiTarget::Sync { .. }
    ) && val == 0
    {
        return None;
    }
    Some(target_to_cmd(&binding.target, val))
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

fn target_to_cmd(target: &MidiTarget, val: u8) -> Command {
    let n = val as f32 / 127.0;
    match *target {
        MidiTarget::Xfader => Command::SetCrossfader {
            value: n * 2.0 - 1.0,
        },
        MidiTarget::Master => Command::SetMaster { value: n },
        MidiTarget::CueGain => Command::SetCueGain { value: n },
        MidiTarget::Fader { deck } => Command::SetChannelFader { deck, value: n },
        MidiTarget::Gain { deck } => Command::SetChannelGain {
            deck,
            db: n * 24.0 - 12.0,
        },
        MidiTarget::Eq { deck, band } => Command::SetEq {
            deck,
            band,
            db: n * 24.0 - 12.0,
        },
        MidiTarget::Filter { deck } => Command::SetChannelFilter {
            deck,
            amount: n * 2.0 - 1.0,
        },
        MidiTarget::Resonance { deck } => Command::SetFilterResonance { deck, resonance: n },
        MidiTarget::Tempo { deck } => Command::SetRate {
            deck,
            rate: 0.88 + n * 0.24,
        },
        MidiTarget::Pitch { deck } => Command::SetPitchSemitones {
            deck,
            semitones: n * 12.0 - 6.0,
        },
        MidiTarget::Play { deck } => Command::PlayPause { deck },
        MidiTarget::Cue { deck, index } => Command::JumpCue { deck, index },
        // Two's complement relative CC: 1..63 forward, 65..127 backward, 0/64 idle.
        MidiTarget::Jog { deck } => Command::Jog {
            deck,
            delta_frames: match val {
                0 | 64 => 0.0,
                1..=63 => val as f32 * 16.0,
                _ => (val as i16 - 128) as f32 * 16.0,
            },
        },
        MidiTarget::Sync { deck } => Command::Sync {
            deck,
            keylock: true,
        },
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
        }
    }

    #[test]
    fn releases_and_malformed_messages_never_trigger_buttons() {
        let map = [binding()];
        assert!(matches!(
            resolve(&map, "one", &[0x90, 60, 127]),
            Some(Command::PlayPause { .. })
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
            Some(Command::PlayPause { deck: DeckId::A })
        ));
        assert!(matches!(
            resolve(&map, "two", &[0x90, 60, 1]),
            Some(Command::PlayPause { deck: DeckId::B })
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
    fn relative_jog_has_neutral_and_symmetric_steps() {
        for (value, expected) in [(0, 0.0), (64, 0.0), (1, 16.0), (127, -16.0)] {
            let Command::Jog { delta_frames, .. } =
                target_to_cmd(&MidiTarget::Jog { deck: DeckId::A }, value)
            else {
                panic!()
            };
            assert_eq!(delta_frames, expected);
        }
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
        assert_eq!(read_map(&path).unwrap(), vec![replacement.clone()]);
        let export = dir.join("export.json");
        hub.export(&export).unwrap();
        hub.remove(0).unwrap();
        assert!(hub.bindings().is_empty());
        hub.import(&export).unwrap();
        assert_eq!(hub.bindings(), vec![replacement]);
        fs::write(&export, br#"{"bindings":[{"kind":"note","channel":20,"number":60,"target":{"type":"master"}}]}"#).unwrap();
        assert!(hub.import(&export).is_err());
        assert_eq!(hub.bindings().len(), 1);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
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
        hub.set_learn(true);
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
            [Command::SetMaster { value: 1.0 }]
        ));
        hub.configure(MidiConfig {
            enabled: false,
            input_ids: None,
        })
        .unwrap();
        fs::remove_file(path).unwrap();
    }
}
