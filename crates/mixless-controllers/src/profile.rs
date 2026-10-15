//! Versioned, portable, declarative profiles. Device-instance identifiers live only in LocalState.
use crate::{MidiBinding, MidiControlMode, MidiError, MidiSourceKind, MidiTarget, error};
pub use mixless_midi::Address as MidiAddress;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

pub const PROFILE_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerPreset {
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub manufacturer: String,
    /// Setup instructions and known coverage limits, shared with exported presets.
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub provenance: Vec<Provenance>,
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    #[serde(default)]
    pub feedback: Vec<FeedbackRule>,
    #[serde(default)]
    pub initialize: Vec<OutputPacket>,
    #[serde(default)]
    pub shutdown: Vec<OutputPacket>,
    #[serde(default)]
    pub audio: Option<AudioRecommendation>,
    #[serde(default)]
    pub verification: Verification,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub community: Option<crate::community::CommunityInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<crate::community::AdapterDefinition>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    #[default]
    Protocol,
    Simulation,
    Hardware,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub url: String,
    pub revision: String,
    pub section: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioRecommendation {
    pub device_name: String,
    pub master: [u16; 2],
    pub cue: [u16; 2],
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "protocol", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeviceMatcher {
    Midi {
        #[serde(default)]
        names: Vec<String>,
    },
    Hid {
        vendor_id: u16,
        product_id: u16,
        interface: Option<i32>,
        usage_page: Option<u16>,
        usage: Option<u16>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub id: String,
    pub matcher: DeviceMatcher,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "protocol", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputSource {
    Midi {
        address: MidiAddress,
    },
    Hid {
        report_id: u8,
        bit_offset: u16,
        bits: u8,
        #[serde(default)]
        signed: bool,
        #[serde(default = "yes")]
        little_endian: bool,
    },
}
fn yes() -> bool {
    true
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub id: String,
    pub endpoint: String,
    pub source: InputSource,
    pub target: MidiTarget,
    #[serde(default)]
    pub mode: ValueMode,
    #[serde(default)]
    pub inverted: bool,
    #[serde(default)]
    pub pickup: bool,
    #[serde(default = "unit")]
    pub sensitivity: f64,
    #[serde(default)]
    pub minimum: Option<f64>,
    #[serde(default)]
    pub maximum: Option<f64>,
    /// Revolutions per second are converted using the current source sample rate.
    #[serde(default)]
    pub ticks_per_revolution: Option<f64>,
    #[serde(default)]
    pub layer: Option<LayerCondition>,
    /// A held modifier updates a device-local layer instead of emitting a control.
    #[serde(default)]
    pub sets_layer: Option<String>,
}
fn unit() -> f64 {
    1.0
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerCondition {
    pub name: String,
    pub active: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueMode {
    #[default]
    Auto,
    Absolute,
    TwosComplement,
    BinaryOffset,
    SignedBit,
    Counter,
    Button,
    Momentary,
    Toggle,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    Output,
    Feature,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputPacket {
    pub endpoint: String,
    pub data: Vec<u8>,
    #[serde(default)]
    pub report: Option<ReportKind>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeedbackSignal {
    Control {
        target: MidiTarget,
    },
    Meter {
        deck: Option<mixless_protocol::DeckId>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackRule {
    pub signal: FeedbackSignal,
    pub packet: OutputPacket,
    /// Bit position includes the HID report-ID byte, or the complete MIDI packet.
    pub bit_offset: u16,
    pub bits: u8,
    #[serde(default = "yes")]
    pub little_endian: bool,
    #[serde(default)]
    pub off: u32,
    pub on: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceAssignment {
    pub device_id: String,
    pub preset_id: String,
    /// Logical endpoint -> local input path/ID. Never exported with a preset.
    pub ports: BTreeMap<String, String>,
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
}
#[derive(Debug, Clone)]
pub struct ImportReport {
    pub preset: ControllerPreset,
    pub conflicts: Vec<String>,
    pub requirements: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalState {
    pub version: u32,
    pub presets: Vec<ControllerPreset>,
    pub assignments: Vec<DeviceAssignment>,
    #[serde(default)]
    pub wildcard: Vec<MidiBinding>,
    #[serde(default)]
    pub legacy_migrated: bool,
}
impl Default for LocalState {
    fn default() -> Self {
        Self {
            version: 1,
            presets: vec![],
            assignments: vec![],
            wildcard: vec![],
            legacy_migrated: false,
        }
    }
}

pub fn new_id() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "user-{:x}-{:x}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
impl ControllerPreset {
    pub fn blank(name: &str) -> Self {
        Self {
            version: 1,
            id: new_id(),
            name: name.trim().into(),
            manufacturer: String::new(),
            notes: String::new(),
            provenance: vec![],
            endpoints: vec![Endpoint {
                id: "main".into(),
                matcher: DeviceMatcher::Midi { names: vec![] },
            }],
            bindings: vec![],
            feedback: vec![],
            initialize: vec![],
            shutdown: vec![],
            audio: None,
            verification: Verification::Protocol,
            community: None,
            adapter: None,
        }
    }
    pub fn validate(&self) -> Result<(), MidiError> {
        crate::community::validate(self)?;
        if self.version != PROFILE_VERSION {
            return Err(error(format!(
                "Unsupported preset version {}",
                self.version
            )));
        }
        if self.id.is_empty()
            || self.id.len() > 200
            || self.name.trim().is_empty()
            || self.name.len() > 200
        {
            return Err(error("Preset needs an ID and a name of at most 200 bytes"));
        }
        if self.bindings.len() > 4096
            || self.notes.len() > 4096
            || self.manufacturer.len() > 200
            || self.provenance.len() > 64
            || self
                .provenance
                .iter()
                .any(|p| p.url.len() > 2048 || p.revision.len() > 200 || p.section.len() > 4096)
            || self.endpoints.len() > 32
            || self.feedback.len() > 2048
            || self.initialize.len() > 128
            || self.shutdown.len() > 128
        {
            return Err(error("Preset exceeds controller limits"));
        }
        let mut endpoints = BTreeSet::new();
        for e in &self.endpoints {
            if e.id.is_empty() || e.id.len() > 200 || !endpoints.insert(e.id.as_str()) {
                return Err(error("Duplicate or empty endpoint"));
            }
        }
        let mut ids = BTreeSet::new();
        let mut sources = Vec::new();
        for b in &self.bindings {
            if b.id.is_empty()
                || b.id.len() > 200
                || !ids.insert(&b.id)
                || !endpoints.contains(b.endpoint.as_str())
                || !MidiTarget::all().contains(&b.target)
            {
                return Err(error("Invalid binding ID, endpoint or target"));
            }
            if !b.sensitivity.is_finite()
                || b.sensitivity <= 0.0
                || b.sensitivity > 1000.0
                || b.ticks_per_revolution
                    .is_some_and(|v| !v.is_finite() || v <= 0.0)
                || b.minimum.is_some_and(|v| !v.is_finite())
                || b.maximum.is_some_and(|v| !v.is_finite())
                || b.minimum.zip(b.maximum).is_some_and(|(a, z)| a >= z)
            {
                return Err(error("Invalid control calibration"));
            }
            if let Some(layer) = &b.layer {
                if layer.name.is_empty()
                    || !self
                        .bindings
                        .iter()
                        .any(|m| m.sets_layer.as_ref() == Some(&layer.name))
                {
                    return Err(error("Layer references an undefined modifier"));
                }
            }
            if b.sets_layer
                .as_ref()
                .is_some_and(|l| l.is_empty() || l.len() > 100)
            {
                return Err(error("Invalid modifier name"));
            }
            if b.mode == ValueMode::Momentary
                && b.sets_layer.is_none()
                && !matches!(
                    b.target,
                    MidiTarget::JogTouch { .. }
                        | MidiTarget::Brake { .. }
                        | MidiTarget::TemporaryCue { .. }
                        | MidiTarget::Cue { .. }
                )
            {
                return Err(error("Momentary requires a hold-capable target"));
            }
            let e = self.endpoints.iter().find(|e| e.id == b.endpoint).unwrap();
            let valid = match (&b.source, &e.matcher) {
                (InputSource::Midi { address }, DeviceMatcher::Midi { .. }) => address.valid(),
                (
                    InputSource::Hid {
                        bit_offset, bits, ..
                    },
                    DeviceMatcher::Hid { .. },
                ) => {
                    *bit_offset >= 8
                        && (1..=32).contains(bits)
                        && (*bit_offset as usize + *bits as usize) <= 4096 * 8
                }
                _ => false,
            };
            if !valid {
                return Err(error("Invalid source or endpoint protocol"));
            }
            if sources.iter().any(|p: &&Binding| {
                p.endpoint == b.endpoint && p.source == b.source && p.layer == b.layer
            }) {
                return Err(error("Duplicate input in the same layer"));
            }
            sources.push(b);
        }
        for p in self
            .initialize
            .iter()
            .chain(&self.shutdown)
            .chain(self.feedback.iter().map(|f| &f.packet))
        {
            let Some(e) = self.endpoints.iter().find(|e| e.id == p.endpoint) else {
                return Err(error("Unknown output endpoint"));
            };
            if p.data.is_empty() || p.data.len() > 4096 {
                return Err(error("Invalid output packet length"));
            }
            if matches!(e.matcher, DeviceMatcher::Midi { .. }) {
                if p.report.is_some()
                    || !(p.data.len() == 3
                        && (0x80..=0xef).contains(&p.data[0])
                        && p.data[1..].iter().all(|b| *b < 128)
                        || p.data.first() == Some(&0xf0)
                            && p.data.last() == Some(&0xf7)
                            && p.data[1..p.data.len() - 1].iter().all(|b| *b < 128))
                {
                    return Err(error("Invalid MIDI output packet"));
                }
            } else if p.report.is_none() {
                return Err(error("HID output needs a report kind"));
            }
        }
        for f in &self.feedback {
            if f.bits == 0
                || f.bits > 32
                || f.bit_offset as usize + f.bits as usize > f.packet.data.len() * 8
                || f.on as u64 > (1u64 << f.bits) - 1
                || f.off as u64 > (1u64 << f.bits) - 1
            {
                return Err(error("Invalid feedback field"));
            }
            if self.endpoints.iter().any(|e| {
                e.id == f.packet.endpoint && matches!(e.matcher, DeviceMatcher::Midi { .. })
            }) {
                if !f.little_endian
                    || f.bit_offset % 8 != 0
                    || f.bits > 7
                    || f.bit_offset as usize / 8
                        >= f.packet.data.len() - usize::from(f.packet.data[0] == 0xf0)
                {
                    return Err(error("MIDI feedback must change one 7-bit data field"));
                }
            }
            if f.bit_offset < 8 {
                return Err(error("Feedback must preserve status/report ID"));
            }
        }
        Ok(())
    }
    pub fn matches_midi(&self, name: &str) -> bool {
        self.endpoints.iter().any(|e|matches!(&e.matcher,DeviceMatcher::Midi{names} if names.iter().any(|n|n.eq_ignore_ascii_case(name))))
    }
    pub fn is_builtin(&self) -> bool {
        self.id.starts_with("builtin-")
    }
}
impl Binding {
    pub fn from_legacy(b: &MidiBinding) -> Self {
        let mut result = Self {
            id: format!(
                "midi-{}",
                blake3::hash(
                    &serde_json::to_vec(&b.midi_address()).expect("MIDI address serialization")
                )
                .to_hex()
            ),
            endpoint: "main".into(),
            source: InputSource::Midi {
                address: b.midi_address(),
            },
            target: b.target.clone(),
            mode: ValueMode::Auto,
            inverted: b.inverted || b.mode == MidiControlMode::AbsoluteInverted,
            pickup: b.pickup,
            sensitivity: b.sensitivity,
            minimum: None,
            maximum: None,
            ticks_per_revolution: b.ticks_per_revolution,
            layer: None,
            sets_layer: None,
        };
        result.mode = match b.mode {
            MidiControlMode::Auto => ValueMode::Auto,
            MidiControlMode::Absolute | MidiControlMode::AbsoluteInverted => ValueMode::Absolute,
            MidiControlMode::RelativeTwosComplement => ValueMode::TwosComplement,
            MidiControlMode::RelativeBinaryOffset => ValueMode::BinaryOffset,
            MidiControlMode::RelativeSignedBit => ValueMode::SignedBit,
        };
        result
    }
    pub fn legacy(&self, device_id: Option<String>) -> Option<MidiBinding> {
        if self.layer.is_some()
            || self.sets_layer.is_some()
            || self.minimum.is_some()
            || self.maximum.is_some()
        {
            return None;
        }
        let (kind, channel, number) = match self.source {
            InputSource::Midi {
                address: MidiAddress::Cc { channel, number },
            } => (MidiSourceKind::Cc, channel, number),
            InputSource::Midi {
                address: MidiAddress::Note { channel, number },
            } => (MidiSourceKind::Note, channel, number),
            InputSource::Midi {
                address: MidiAddress::PitchBend { channel },
            } => (MidiSourceKind::Cc, channel, 0),
            InputSource::Midi {
                address: MidiAddress::Cc14 { channel, msb, .. },
            } => (MidiSourceKind::Cc, channel, msb),
            _ => return None,
        };
        let mode = match self.mode {
            ValueMode::Auto => MidiControlMode::Auto,
            ValueMode::Absolute => MidiControlMode::Absolute,
            ValueMode::TwosComplement => MidiControlMode::RelativeTwosComplement,
            ValueMode::BinaryOffset => MidiControlMode::RelativeBinaryOffset,
            ValueMode::SignedBit => MidiControlMode::RelativeSignedBit,
            _ => return None,
        };
        let mut result = MidiBinding {
            address: None,
            inverted: self.inverted,
            sensitivity: self.sensitivity,
            ticks_per_revolution: self.ticks_per_revolution,
            pickup: self.pickup,
            device_id,
            kind,
            channel,
            number,
            target: self.target.clone(),
            mode,
        };
        if let InputSource::Midi { address } = &self.source {
            result.set_address(address.clone());
        }
        Some(result)
    }
}

pub(crate) fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), MidiError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(error)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", new_id()));
    let result = (|| {
        use std::io::Write;
        let bytes = serde_json::to_vec_pretty(value).map_err(error)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(error)?;
        file.write_all(&bytes).map_err(error)?;
        file.sync_all().map_err(error)?;
        fs::rename(&tmp, path).map_err(error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub fn read_preset(path: &Path) -> Result<ControllerPreset, MidiError> {
    let p: ControllerPreset = read_document(path, 4 * 1024 * 1024)?;
    p.validate()?;
    Ok(p)
}
pub fn read_document<T: serde::de::DeserializeOwned>(
    path: &Path,
    limit: usize,
) -> Result<T, MidiError> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(error)?;
    if !file.metadata().map_err(error)?.is_file() {
        return Err(error("Expected a regular JSON file"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() > limit {
        return Err(error(format!("Document exceeds {} bytes", limit)));
    }
    serde_json::from_slice(&bytes).map_err(error)
}
pub(crate) struct PresetStore {
    pub path: PathBuf,
    pub state: LocalState,
}
impl PresetStore {
    pub fn open(legacy: &Path) -> Result<Self, MidiError> {
        let path = legacy.with_extension("controllers.json");
        let mut store = Self {
            state: if path.exists() {
                serde_json::from_slice(&fs::read(&path).map_err(error)?).map_err(error)?
            } else {
                LocalState::default()
            },
            path,
        };
        if store.state.version != 1 {
            return Err(error("Unsupported controller settings version"));
        }
        for p in &store.state.presets {
            p.validate()?;
        }
        if !store.state.legacy_migrated && legacy.exists() {
            let map = crate::read_map(legacy)?;
            let backup = legacy.with_extension("pre-controllers.backup.json");
            if !backup.exists() {
                let data = fs::read(legacy).map_err(error)?;
                use std::io::Write;
                let mut f = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(backup)
                    .map_err(error)?;
                f.write_all(&data).map_err(error)?;
                f.sync_all().map_err(error)?;
            }
            let mut groups: BTreeMap<Option<String>, Vec<MidiBinding>> = BTreeMap::new();
            for b in map {
                groups.entry(b.device_id.clone()).or_default().push(b);
            }
            for (device, bindings) in groups {
                if let Some(device) = device {
                    let mut p = ControllerPreset::blank("Migrated MIDI mapping");
                    p.bindings = bindings.iter().map(Binding::from_legacy).collect();
                    store.state.assignments.push(DeviceAssignment {
                        device_id: device.clone(),
                        preset_id: p.id.clone(),
                        ports: BTreeMap::from([("main".into(), device)]),
                        outputs: BTreeMap::new(),
                    });
                    store.state.presets.push(p);
                } else {
                    store.state.wildcard = bindings;
                }
            }
            store.state.legacy_migrated = true;
            store.persist()?;
        }
        Ok(store)
    }
    pub fn all(&self) -> Vec<ControllerPreset> {
        let mut all = builtin_presets();
        all.extend(self.state.presets.clone());
        all
    }
    pub fn preset(&self, id: &str) -> Option<ControllerPreset> {
        self.state
            .presets
            .iter()
            .find(|p| p.id == id)
            .or_else(|| builtin_catalogue().iter().find(|p| p.id == id))
            .cloned()
    }
    pub fn persist(&self) -> Result<(), MidiError> {
        atomic_json(&self.path, &self.state)
    }
    pub fn edit<T>(
        &mut self,
        f: impl FnOnce(&mut LocalState) -> Result<T, MidiError>,
    ) -> Result<T, MidiError> {
        let before = self.state.clone();
        let result = f(&mut self.state).and_then(|v| self.persist().map(|_| v));
        if result.is_err() {
            self.state = before;
        }
        result
    }
    pub fn flattened(&self) -> Vec<MidiBinding> {
        let mut result = self.state.wildcard.clone();
        for a in &self.state.assignments {
            if let Some(p) = self.preset(&a.preset_id) {
                for b in &p.bindings {
                    if let Some(id) = a.ports.get(&b.endpoint) {
                        if let Some(b) = b.legacy(Some(id.clone())) {
                            result.push(b);
                        }
                    }
                }
            }
        }
        result
    }
}

pub fn builtin_presets() -> Vec<ControllerPreset> {
    builtin_catalogue().to_vec()
}

/// A canonical model label only when the observed name matches one built-in.
/// This is presentation metadata; it never assigns or opens a controller.
pub fn builtin_model_name(name: &str) -> Option<&'static str> {
    let mut matches = builtin_catalogue().iter().filter(|p| p.matches_midi(name));
    let preset = matches.next()?;
    matches.next().is_none().then_some(preset.name.as_str())
}
fn builtin_catalogue() -> &'static [ControllerPreset] {
    static PRESETS: std::sync::OnceLock<Vec<ControllerPreset>> = std::sync::OnceLock::new();
    PRESETS.get_or_init(crate::catalogue::catalogue)
}

/// Internal-mixer output routing from Traktor Pro 4.4.1's embedded configuration.
/// Exact source device names only; see research/ni/traktor-4.4.1-audit.json.
/// This proposes audio channels and does not establish controller compatibility.
pub fn ni_audio_recommendation(model: &str) -> Option<AudioRecommendation> {
    matches!(
        model,
        "Traktor MX2"
            | "Traktor Z1 MK2"
            | "Traktor Kontrol Z1"
            | "Traktor Kontrol Z2"
            | "Traktor Kontrol S2"
            | "Traktor Kontrol S2 MK2"
            | "Traktor Kontrol S2 MK3"
            | "Traktor Kontrol S3"
            | "Traktor Kontrol S4"
            | "Traktor Kontrol S4 MK2"
            | "Traktor Kontrol S4 MK3"
            | "Traktor Kontrol S5"
            | "Traktor Kontrol S8"
    )
    .then(|| AudioRecommendation {
        device_name: model.into(),
        master: [0, 1],
        cue: [2, 3],
    })
}
