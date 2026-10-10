use super::*;
use crate::profile::{atomic_json, read_preset, PresetStore};
use crate::runtime::{Runtime, RuntimeShared};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// A timestamp is captured in the transport callback, before routing or application work.
#[derive(Debug, Clone)]
pub struct ControlEvent {
    pub device_id: String,
    pub received_at: std::time::Instant,
    pub action: MidiAction,
}
pub type ActionHandler = Arc<dyn Fn(ControlEvent) -> bool + Send + Sync>;
pub type SnapshotReader = Arc<dyn Fn() -> mixless_protocol::EngineSnapshot + Send + Sync>;

pub struct ControllerManager {
    pub(crate) bindings: Arc<Mutex<Vec<MidiBinding>>>,
    pub(crate) input: Arc<Mutex<InputState>>,
    pub(crate) path: PathBuf,
    pub(crate) store: Arc<Mutex<PresetStore>>,
    pub(crate) shared: Arc<RuntimeShared>,
    runtime: Runtime,
}
/// Compatibility name for callers of the previous input-only manager.
pub type MidiHub = ControllerManager;
impl ControllerManager {
    pub fn start(path: PathBuf) -> Result<Self, MidiError> {
        Self::start_with_config(path, MidiConfig::default())
    }
    pub fn start_with_config(path: PathBuf, config: MidiConfig) -> Result<Self, MidiError> {
        let store = PresetStore::open(&path)?;
        let bindings = Arc::new(Mutex::new(store.flattened()));
        let input = Arc::new(Mutex::new(InputState::default()));
        let path = store.path.clone();
        let store = Arc::new(Mutex::new(store));
        let shared = Arc::new(RuntimeShared {
            config: Mutex::new(config),
            store: store.clone(),
            input: input.clone(),
            devices: Mutex::new(vec![]),
            last: Mutex::new(None),
            handler: Mutex::new(None),
            snapshot: Mutex::new(None),
            notify: Mutex::new(None),
            revision: AtomicU64::new(1),
            latencies: Mutex::new(Vec::new()),
            output_errors: Mutex::new(Default::default()),
            adapter_errors: Mutex::new(Default::default()),
            recording: Mutex::new(None),
        });
        let runtime = Runtime::start(shared.clone());
        Ok(Self {
            bindings,
            input,
            path,
            store,
            shared,
            runtime,
        })
    }
    pub fn configure(&self, config: MidiConfig) -> Result<(), MidiError> {
        self.set_learn(false);
        {
            let mut i = self.input.lock().unwrap();
            i.queued.retain(|a| a.value == MidiValue::Release);
            i.release_held();
        }
        *self.shared.config.lock().unwrap() = config;
        self.runtime.wake();
        Ok(())
    }
    pub fn reconnect(&self) -> Result<(), MidiError> {
        self.runtime.wake();
        Ok(())
    }
    pub fn config(&self) -> MidiConfig {
        self.shared.config.lock().unwrap().clone()
    }
    pub fn devices(&self) -> Vec<ControllerDevice> {
        self.shared.devices.lock().unwrap().clone()
    }
    pub fn read_hid_feature(
        &self,
        device: &str,
        report: u8,
        length: usize,
    ) -> Result<Vec<u8>, MidiError> {
        self.runtime
            .read_feature(device.into(), report, length)
            .map_err(error)
    }
    pub fn adapter_error(&self, device: &str) -> Option<String> {
        let primary = self
            .assignments()
            .into_iter()
            .find(|a| a.ports.values().any(|id| id == device))
            .map(|a| a.device_id)
            .unwrap_or_else(|| device.into());
        self.shared
            .adapter_errors
            .lock()
            .unwrap()
            .get(&primary)
            .cloned()
    }
    pub fn feedback_error(&self, device: &str) -> Option<String> {
        if let Some(e) = self.shared.adapter_errors.lock().unwrap().get(device) {
            return Some(e.clone());
        }
        self.shared
            .output_errors
            .lock()
            .unwrap()
            .get(device)
            .cloned()
    }
    pub fn storage_path(&self) -> &Path {
        &self.path
    }
    /// Start before assigning/reconnecting. Captures stay in memory until explicitly exported.
    pub fn start_recording(
        &self,
        device: &str,
        preset: &str,
        firmware: &str,
        os: &str,
    ) -> Result<(), MidiError> {
        let p = self
            .preset(preset)
            .ok_or_else(|| error("Preset unavailable"))?;
        let mut recording = self.shared.recording.lock().unwrap();
        if recording.is_some() {
            return Err(error("A controller capture is already running"));
        }
        if self.assignments().iter().any(|a| a.device_id == device) {
            return Err(error(
                "Start recording before applying the preset, so initialization is captured",
            ));
        }
        *recording = Some(crate::runtime::recording::Recorder::new(
            device.into(),
            crate::replay::ReportTrace {
                version: 1,
                preset_digest: crate::community::content_digest(&p),
                model: p.name,
                firmware: firmware.into(),
                os: os.into(),
                truncated: false,
                frames: vec![],
            },
        ));
        Ok(())
    }
    pub fn stop_recording(&self) -> Result<crate::replay::ReportTrace, MidiError> {
        self.runtime.stop_recording().map_err(error)
    }
    pub fn last_packet(&self) -> Option<ReceivedPacket> {
        self.shared.last.lock().unwrap().clone()
    }
    pub fn set_handler(&self, handler: ActionHandler, snapshot: SnapshotReader) {
        *self.shared.handler.lock().unwrap() = Some(handler);
        *self.shared.snapshot.lock().unwrap() = Some(snapshot);
        self.runtime.wake();
    }
    pub fn set_notify(&self, notify: Arc<dyn Fn() + Send + Sync>) {
        *self.shared.notify.lock().unwrap() = Some(notify);
    }
    pub fn latency_percentiles(&self) -> Option<[u64; 3]> {
        let mut samples = self.shared.latencies.lock().unwrap().clone();
        if samples.is_empty() {
            return None;
        }
        samples.sort_unstable();
        Some([
            samples[samples.len() / 2],
            samples[(samples.len() - 1) * 95 / 100],
            samples[(samples.len() - 1) * 99 / 100],
        ])
    }
    fn changed(&self) {
        *self.bindings.lock().unwrap() = self.store.lock().unwrap().flattened();
        self.shared.revision.fetch_add(1, Ordering::Release);
        self.runtime.wake();
    }
    pub fn presets(&self) -> Vec<ControllerPreset> {
        self.store.lock().unwrap().all()
    }
    pub fn preset(&self, id: &str) -> Option<ControllerPreset> {
        self.store.lock().unwrap().preset(id)
    }
    pub fn assignments(&self) -> Vec<DeviceAssignment> {
        self.store.lock().unwrap().state.assignments.clone()
    }
    pub fn save_preset(&self, mut preset: ControllerPreset) -> Result<String, MidiError> {
        preset.validate()?;
        if preset.adapter.is_some() {
            crate::adapter::AdapterInstance::new(&preset)?;
        }
        preset.verification = Verification::Protocol;
        // Built-ins are always copied; their catalogue data is immutable.
        if preset.is_builtin() {
            preset.id = new_id();
            preset.name = format!("{} — Custom", preset.name);
            preset.verification = Verification::Protocol;
        }
        let id = preset.id.clone();
        self.store.lock().unwrap().edit(|state| {
            if let Some(p) = state.presets.iter_mut().find(|p| p.id == id) {
                *p = preset;
            } else {
                state.presets.push(preset);
            }
            Ok(())
        })?;
        self.changed();
        Ok(id)
    }
    pub fn save_device_preset(
        &self,
        mut p: ControllerPreset,
        device: Option<&str>,
    ) -> Result<String, MidiError> {
        let assigned = self.assignments();
        if assigned
            .iter()
            .any(|a| a.preset_id == p.id && Some(a.device_id.as_str()) != device)
        {
            p.id = new_id();
            p.name = format!("{} — Custom", p.name);
            p.verification = Verification::Protocol;
        }
        // Saving never changes assignments. Applying the new copy is explicit.
        self.save_preset(p)
    }
    pub fn duplicate_preset(&self, id: &str, name: &str) -> Result<String, MidiError> {
        let mut p = self.preset(id).ok_or_else(|| error("Preset unavailable"))?;
        p.id = new_id();
        p.name = name.trim().into();
        p.verification = Verification::Protocol;
        self.save_preset(p)
    }
    pub fn rename_preset(&self, id: &str, name: &str) -> Result<(), MidiError> {
        let mut p = self.preset(id).ok_or_else(|| error("Preset unavailable"))?;
        if p.is_builtin() {
            return Err(error("Duplicate a built-in preset before renaming"));
        }
        p.name = name.trim().into();
        self.save_preset(p)?;
        Ok(())
    }
    pub fn delete_preset(&self, id: &str) -> Result<(), MidiError> {
        if id.starts_with("builtin-") {
            return Err(error("Built-in presets are read-only"));
        }
        self.store.lock().unwrap().edit(|s| {
            if s.assignments.iter().any(|a| a.preset_id == id) {
                return Err(error(
                    "Unassign this preset from its devices before deleting it",
                ));
            }
            s.presets.retain(|p| p.id != id);
            Ok(())
        })?;
        self.changed();
        Ok(())
    }
    pub fn preview_import(&self, path: &Path) -> Result<ImportReport, MidiError> {
        let preset = read_preset(path)?;
        if preset.adapter.is_some() {
            crate::adapter::AdapterInstance::new(&preset)?;
        }
        let all = self.presets();
        let conflicts = all
            .iter()
            .filter(|p| p.id == preset.id || p.name == preset.name)
            .map(|p| {
                format!(
                    "{} already exists; import will create a separate copy",
                    p.name
                )
            })
            .collect();
        let requirements = preset
            .endpoints
            .iter()
            .map(|e| match &e.matcher {
                DeviceMatcher::Midi { names } => format!("{}: MIDI {}", e.id, names.join(", ")),
                DeviceMatcher::Hid {
                    vendor_id,
                    product_id,
                    ..
                } => format!("{}: HID {:04X}:{:04X}", e.id, vendor_id, product_id),
            })
            .collect();
        Ok(ImportReport {
            preset,
            conflicts,
            requirements,
        })
    }
    pub fn accept_import(&self, report: ImportReport) -> Result<String, MidiError> {
        let mut p = report.preset;
        p.id = new_id();
        p.verification = Verification::Protocol;
        self.save_preset(p)
    }
    pub fn export_preset(&self, id: &str, path: &Path) -> Result<(), MidiError> {
        let p = self
            .preset(id)
            .ok_or_else(|| error("Choose a preset to export"))?;
        atomic_json(path, &p)
    }
    /// Applies only to the explicitly selected instance. Additional ports are explicit assignments.
    pub fn assign(&self, assignment: DeviceAssignment) -> Result<(), MidiError> {
        let p = self
            .preset(&assignment.preset_id)
            .ok_or_else(|| error("Preset unavailable"))?;
        if assignment.device_id.is_empty()
            || p.endpoints
                .iter()
                .any(|e| !assignment.ports.contains_key(&e.id))
            || assignment.ports.len() != p.endpoints.len()
        {
            return Err(error("Choose a device for every logical input endpoint"));
        }
        if assignment.ports.values().any(|v| v.is_empty()) {
            return Err(error("An input endpoint cannot be empty"));
        }
        let ports: std::collections::BTreeSet<_> = assignment.ports.values().cloned().collect();
        if ports.len() != assignment.ports.len()
            || !ports.contains(&assignment.device_id)
            || assignment
                .outputs
                .keys()
                .any(|e| !assignment.ports.contains_key(e))
        {
            return Err(error(
                "Each endpoint needs a different input; the primary device must be included",
            ));
        }
        for (endpoint, id) in &assignment.ports {
            if let Some(d) = self.devices().iter().find(|d| &d.id == id) {
                if !d.compatible(
                    &p.endpoints
                        .iter()
                        .find(|e| &e.id == endpoint)
                        .unwrap()
                        .matcher,
                ) {
                    return Err(error("Endpoint protocol does not match device"));
                }
            }
        }
        let primary = assignment.device_id.clone();
        let reapply = self.assignments().contains(&assignment);
        self.store.lock().unwrap().edit(|s| {
            if s.assignments.iter().any(|a| {
                a.device_id != assignment.device_id && a.ports.values().any(|id| ports.contains(id))
            }) {
                return Err(error("Input is already assigned to another controller"));
            }
            s.assignments
                .retain(|a| a.device_id != assignment.device_id);
            s.assignments.push(assignment);
            Ok(())
        })?;
        self.changed();
        if reapply {
            self.runtime.restart(primary);
        }
        Ok(())
    }
    pub fn unassign(&self, device: &str) -> Result<(), MidiError> {
        self.store.lock().unwrap().edit(|s| {
            s.assignments.retain(|a| a.device_id != device);
            Ok(())
        })?;
        self.changed();
        Ok(())
    }
    pub fn apply_preset_to(&self, id: &str, device: &str) -> Result<(), MidiError> {
        let p = self.preset(id).ok_or_else(|| error("Preset unavailable"))?;
        if p.endpoints.len() != 1 {
            return Err(error(
                "Assign each port of this preset in the endpoint editor",
            ));
        }
        let d = self
            .devices()
            .into_iter()
            .find(|d| d.id == device)
            .ok_or_else(|| {
                error("Select a connected device; offline presets can be saved without applying")
            })?;
        if d.state == ConnectionState::Offline {
            return Err(error("Select a connected device"));
        }
        if !d.compatible(&p.endpoints[0].matcher) {
            return Err(error(
                "Preset protocol or hardware identity does not match the selected device",
            ));
        }
        self.assign(DeviceAssignment {
            device_id: device.into(),
            preset_id: id.into(),
            ports: BTreeMap::from([(p.endpoints[0].id.clone(), device.into())]),
            outputs: BTreeMap::new(),
        })
    }
    pub fn set_learn(&self, on: bool) {
        let mut i = self.input.lock().unwrap();
        if on {
            i.start_learning(None);
        } else {
            i.learning = false;
            i.learn_target = None;
            i.learn_device = None;
            i.learned = None;
            i.release_held();
        }
        drop(i);
        self.runtime.wake();
    }
    pub fn learn_target(&self, target: MidiTarget) -> Result<(), MidiError> {
        self.learn_target_for(target, None)
    }
    pub fn learn_target_for(
        &self,
        target: MidiTarget,
        device: Option<String>,
    ) -> Result<(), MidiError> {
        if !self.devices().iter().any(|d| {
            d.state == ConnectionState::Connected
                && !d.hid
                && device.as_ref().is_none_or(|id| id == &d.id)
        }) {
            return Err(error("No active MIDI input"));
        }
        {
            let mut input = self.input.lock().unwrap();
            input.start_learning(Some(target));
            input.learn_device = device;
        }
        self.runtime.wake();
        Ok(())
    }
    pub fn learned(&self) -> Option<MidiMessage> {
        self.input.lock().unwrap().learned.clone()
    }
    pub fn last_message(&self) -> Option<MidiMessage> {
        self.input.lock().unwrap().last.clone()
    }
    pub fn last_seen(&self) -> Option<(MidiSourceKind, u8, u8, u8)> {
        self.last_message()
            .map(|m| (m.kind, m.channel, m.number, m.value))
    }
    pub fn bind(&self, target: MidiTarget) -> Result<(), MidiError> {
        let m = self
            .learned()
            .ok_or_else(|| error("Waiting for MIDI input"))?;
        self.upsert(MidiBinding {
            device_id: Some(m.device_id),
            kind: m.kind,
            channel: m.channel,
            number: m.number,
            target,
            mode: MidiControlMode::Auto,
        })?;
        self.set_learn(false);
        Ok(())
    }
    pub fn bindings(&self) -> Vec<MidiBinding> {
        self.bindings.lock().unwrap().clone()
    }
    pub fn upsert(&self, binding: MidiBinding) -> Result<(), MidiError> {
        validate(&binding)?;
        self.edit_map(|b| {
            b.retain(|b| !same_source(b, &binding));
            b.push(binding);
        })
    }
    pub fn remove(&self, index: usize) -> Result<(), MidiError> {
        self.edit_map(|b| {
            if index < b.len() {
                b.remove(index);
            }
        })
    }
    pub fn replace(&self, index: usize, binding: MidiBinding) -> Result<(), MidiError> {
        validate(&binding)?;
        self.edit_map(|b| {
            if index < b.len() {
                b.remove(index);
            }
            b.retain(|b| !same_source(b, &binding));
            b.push(binding);
        })
    }
    fn edit_map(&self, edit: impl FnOnce(&mut Vec<MidiBinding>)) -> Result<(), MidiError> {
        let before = self.bindings();
        let mut next = before.clone();
        edit(&mut next);
        let all = self.presets();
        self.store.lock().unwrap().edit(|s| {
            s.wildcard = next
                .iter()
                .filter(|b| b.device_id.is_none())
                .cloned()
                .collect();
            let mut devices = std::collections::BTreeSet::new();
            for b in before.iter().chain(&next) {
                if let Some(id) = &b.device_id {
                    devices.insert(id.clone());
                }
            }
            for device in devices {
                let old: Vec<_> = before
                    .iter()
                    .filter(|b| b.device_id.as_ref() == Some(&device))
                    .collect();
                let new: Vec<_> = next
                    .iter()
                    .filter(|b| b.device_id.as_ref() == Some(&device))
                    .collect();
                if old == new {
                    continue;
                }
                let assignment = s
                    .assignments
                    .iter()
                    .position(|a| a.ports.values().any(|v| v == &device));
                let mut p = assignment
                    .and_then(|i| all.iter().find(|p| p.id == s.assignments[i].preset_id))
                    .cloned()
                    .unwrap_or_else(|| ControllerPreset::blank("Custom MIDI mapping"));
                let endpoint = assignment
                    .and_then(|i| {
                        s.assignments[i]
                            .ports
                            .iter()
                            .find(|(_, v)| *v == &device)
                            .map(|(k, _)| k.clone())
                    })
                    .unwrap_or("main".into());
                let shared = s.assignments.iter().filter(|a| a.preset_id == p.id).count() > 1;
                if p.is_builtin() || shared {
                    p.id = new_id();
                    p.name = format!("{} — Custom", p.name);
                }
                // Keep high-resolution, layered and HID bindings when editing a simple row.
                p.bindings
                    .retain(|b| b.endpoint != endpoint || b.legacy(None).is_none());
                for b in new {
                    let previous = assignment
                        .and_then(|i| all.iter().find(|p| p.id == s.assignments[i].preset_id))
                        .and_then(|p| {
                            p.bindings
                                .iter()
                                .find(|v| v.legacy(Some(device.clone())).as_ref() == Some(b))
                        })
                        .cloned();
                    let mut n = previous.unwrap_or_else(|| Binding::from_legacy(b));
                    n.endpoint = endpoint.clone();
                    p.bindings.push(n);
                }
                p.validate()?;
                if let Some(i) = assignment {
                    s.assignments[i].preset_id = p.id.clone();
                } else {
                    s.assignments.push(DeviceAssignment {
                        device_id: device.clone(),
                        preset_id: p.id.clone(),
                        ports: BTreeMap::from([("main".into(), device)]),
                        outputs: BTreeMap::new(),
                    });
                }
                if let Some(v) = s.presets.iter_mut().find(|v| v.id == p.id) {
                    *v = p;
                } else {
                    s.presets.push(p);
                }
            }
            Ok(())
        })?;
        {
            let mut i = self.input.lock().unwrap();
            i.queued.retain(|a| a.value == MidiValue::Release);
            i.release_held();
        }
        self.changed();
        Ok(())
    }
    /// Legacy imports remain available only to migration/tests; the product UI imports native profiles.
    pub fn import(&self, path: &Path) -> Result<(), MidiError> {
        let bindings = read_map(path)?;
        self.edit_map(|b| *b = bindings)
    }
    pub fn export(&self, path: &Path) -> Result<(), MidiError> {
        save_map(path, &self.bindings())
    }
    pub fn drain(&self) -> Vec<MidiAction> {
        std::mem::take(&mut self.input.lock().unwrap().queued)
    }
    pub fn map_message(&self, msg: &[u8]) -> Option<MidiAction> {
        resolve(&self.bindings.lock().ok()?, "", msg)
    }
    pub fn poll_and_apply<F: FnMut(MidiAction)>(&self, raw: &[u8], mut apply: F) {
        if let Some(a) = self.map_message(raw) {
            apply(a);
        }
    }
}
