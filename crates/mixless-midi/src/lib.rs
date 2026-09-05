//! MIDI input → Command. Audio thread never sees midir.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use midir::{Ignore, MidiInput, MidiInputConnection};
use mixless_protocol::{Command, DeckId, EqBand};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MidiError {
    #[error("{0}")]
    Init(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiSourceKind {
    Cc,
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MidiTarget {
    Xfader,
    Master,
    Fader { deck: DeckId },
    Gain { deck: DeckId },
    Eq { deck: DeckId, band: EqBand },
    Filter { deck: DeckId },
    Play { deck: DeckId },
    Cue { deck: DeckId, index: u8 },
    Jog { deck: DeckId },
    Sync { deck: DeckId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiBinding {
    pub kind: MidiSourceKind,
    pub channel: u8,
    pub number: u8,
    pub target: MidiTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MidiMapFile {
    pub bindings: Vec<MidiBinding>,
}

pub struct MidiHub {
    bindings: Arc<Mutex<Vec<MidiBinding>>>,
    learn: Arc<AtomicBool>,
    last: Arc<Mutex<Option<(MidiSourceKind, u8, u8, u8)>>>,
    queued: Arc<Mutex<Vec<Command>>>,
    path: PathBuf,
    _conns: Mutex<Vec<MidiInputConnection<()>>>,
}

impl MidiHub {
    pub fn start(map_path: PathBuf) -> Result<Self, MidiError> {
        let bindings = load_map(&map_path);
        let hub = Self {
            bindings: Arc::new(Mutex::new(bindings)),
            learn: Arc::new(AtomicBool::new(false)),
            last: Arc::new(Mutex::new(None)),
            queued: Arc::new(Mutex::new(Vec::new())),
            path: map_path,
            _conns: Mutex::new(Vec::new()),
        };
        hub.reconnect()?;
        Ok(hub)
    }

    pub fn reconnect(&self) -> Result<(), MidiError> {
        let probe = MidiInput::new("mixless-probe").map_err(|e| MidiError::Init(e.to_string()))?;
        let n = probe.ports().len();
        let mut conns = self._conns.lock().expect("midi");
        conns.clear();
        for i in 0..n {
            let mut midi = MidiInput::new("mixless").map_err(|e| MidiError::Init(e.to_string()))?;
            midi.ignore(Ignore::None);
            let ports = midi.ports();
            let Some(port) = ports.get(i) else {
                continue;
            };
            let name = midi.port_name(port).unwrap_or_else(|_| "unknown".into());
            let last = Arc::clone(&self.last);
            let learn = Arc::clone(&self.learn);
            let queued = Arc::clone(&self.queued);
            let bindings = Arc::clone(&self.bindings);
            match midi.connect(
                port,
                &format!("mixless-{name}"),
                move |_ts, msg, _| {
                    if let Some(parsed) = parse_msg(msg) {
                        if learn.load(Ordering::Relaxed) {
                            if let Ok(mut g) = last.lock() {
                                *g = Some(parsed);
                            }
                        }
                        if let Some(cmd) = resolve(&bindings, msg) {
                            if let Ok(mut q) = queued.lock() {
                                q.push(cmd);
                            }
                        }
                    }
                },
                (),
            ) {
                Ok(c) => conns.push(c),
                Err(e) => tracing::warn!("midi connect {name}: {e}"),
            }
        }
        Ok(())
    }

    pub fn set_learn(&self, on: bool) {
        self.learn.store(on, Ordering::Relaxed);
    }

    pub fn last_seen(&self) -> Option<(MidiSourceKind, u8, u8, u8)> {
        self.last.lock().ok().and_then(|g| *g)
    }

    pub fn bind(&self, target: MidiTarget) -> Result<(), MidiError> {
        let Some((kind, ch, num, _)) = self.last_seen() else {
            return Err(MidiError::Init("no MIDI received while learning".into()));
        };
        let mut g = self.bindings.lock().expect("map");
        g.retain(|b| !(b.kind == kind && b.channel == ch && b.number == num));
        g.push(MidiBinding {
            kind,
            channel: ch,
            number: num,
            target,
        });
        save_map(&self.path, &g);
        Ok(())
    }

    pub fn bindings(&self) -> Vec<MidiBinding> {
        self.bindings.lock().expect("map").clone()
    }

    pub fn drain(&self) -> Vec<Command> {
        self.queued.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
    }

    pub fn map_message(&self, msg: &[u8]) -> Option<Command> {
        resolve(&self.bindings, msg)
    }

    pub fn poll_and_apply<F: FnMut(Command)>(&self, raw: &[u8], mut apply: F) {
        if let Some(cmd) = self.map_message(raw) {
            apply(cmd);
        }
    }
}

fn resolve(bindings: &Mutex<Vec<MidiBinding>>, msg: &[u8]) -> Option<Command> {
    let (kind, ch, num, val) = parse_msg(msg)?;
    if kind == MidiSourceKind::Note && val == 0 {
        return None;
    }
    let g = bindings.lock().ok()?;
    let b = g
        .iter()
        .find(|b| b.kind == kind && b.channel == ch && b.number == num)?;
    Some(target_to_cmd(&b.target, val, kind))
}

fn parse_msg(msg: &[u8]) -> Option<(MidiSourceKind, u8, u8, u8)> {
    if msg.len() < 2 {
        return None;
    }
    let st = msg[0];
    let ch = st & 0x0f;
    match st & 0xf0 {
        0x90 if msg.len() >= 3 && msg[2] > 0 => Some((MidiSourceKind::Note, ch, msg[1], msg[2])),
        0x80 => Some((MidiSourceKind::Note, ch, msg[1], 0)),
        0xb0 if msg.len() >= 3 => Some((MidiSourceKind::Cc, ch, msg[1], msg[2])),
        _ => None,
    }
}

fn target_to_cmd(t: &MidiTarget, val: u8, kind: MidiSourceKind) -> Command {
    let n = val as f32 / 127.0;
    match t {
        MidiTarget::Xfader => Command::SetCrossfader { value: n * 2.0 - 1.0 },
        MidiTarget::Master => Command::SetMaster { value: n },
        MidiTarget::Fader { deck } => Command::SetChannelFader { deck: *deck, value: n },
        MidiTarget::Gain { deck } => Command::SetChannelGain {
            deck: *deck,
            db: n * 24.0 - 12.0,
        },
        MidiTarget::Eq { deck, band } => Command::SetEq {
            deck: *deck,
            band: *band,
            db: n * 24.0 - 12.0,
        },
        MidiTarget::Filter { deck } => Command::SetChannelFilter {
            deck: *deck,
            amount: n * 2.0 - 1.0,
        },
        MidiTarget::Play { deck } => Command::PlayPause { deck: *deck },
        MidiTarget::Cue { deck, index } => {
            if kind == MidiSourceKind::Note && val == 0 {
                Command::PlayPause { deck: *deck } // ignore note-off as no-op via empty? use Jump only on press
            } else {
                Command::JumpCue {
                    deck: *deck,
                    index: *index,
                }
            }
        }
        MidiTarget::Jog { deck } => Command::Jog {
            deck: *deck,
            delta_frames: (n - 0.5) * 800.0,
        },
        MidiTarget::Sync { deck } => Command::Sync {
            deck: *deck,
            keylock: true,
        },
    }
}

fn load_map(path: &Path) -> Vec<MidiBinding> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<MidiMapFile>(&s).ok())
        .map(|f| f.bindings)
        .unwrap_or_default()
}

fn save_map(path: &Path, bindings: &[MidiBinding]) {
    if let Some(p) = path.parent() {
        let _ = fs::create_dir_all(p);
    }
    let _ = fs::write(
        path,
        serde_json::to_string_pretty(&MidiMapFile {
            bindings: bindings.to_vec(),
        })
        .unwrap_or_else(|_| "{}".into()),
    );
}
