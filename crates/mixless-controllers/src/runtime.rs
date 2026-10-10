use crate::profile::PresetStore;
use crate::router::{target_value, write_report_field, Router};
use crate::*;
use mixless_midi::midir::{
    Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Discovered,
    Connected,
    Offline,
    Error(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerDevice {
    pub id: String,
    pub name: String,
    pub hid: bool,
    pub state: ConnectionState,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub interface: Option<i32>,
    pub usage_page: Option<u16>,
    pub usage: Option<u16>,
}
impl ControllerDevice {
    pub fn compatible(&self, m: &DeviceMatcher) -> bool {
        match m {
            DeviceMatcher::Midi { .. } => !self.hid,
            DeviceMatcher::Hid {
                vendor_id,
                product_id,
                interface,
                usage_page,
                usage,
            } => {
                self.hid
                    && self.vendor_id == Some(*vendor_id)
                    && self.product_id == Some(*product_id)
                    && interface.is_none_or(|v| self.interface == Some(v))
                    && usage_page.is_none_or(|v| self.usage_page == Some(v))
                    && usage.is_none_or(|v| self.usage == Some(v))
            }
        }
    }
}
#[derive(Debug, Clone)]
pub struct ReceivedPacket {
    pub device_id: String,
    pub hid: bool,
    pub data: Vec<u8>,
    pub received_at: Instant,
}
pub(crate) struct RuntimeShared {
    pub config: Mutex<MidiConfig>,
    pub store: Arc<Mutex<PresetStore>>,
    pub input: Arc<Mutex<InputState>>,
    pub devices: Mutex<Vec<ControllerDevice>>,
    pub last: Mutex<Option<ReceivedPacket>>,
    pub handler: Mutex<Option<ActionHandler>>,
    pub snapshot: Mutex<Option<SnapshotReader>>,
    pub notify: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    pub revision: AtomicU64,
    pub latencies: Mutex<Vec<u64>>,
    pub output_errors: Mutex<HashMap<String, String>>,
}
enum Event {
    Packet(ReceivedPacket, u64),
    Feature(String, u8, usize, mpsc::Sender<Result<Vec<u8>, String>>),
    Wake,
    Stop,
}
pub(crate) struct Runtime {
    tx: SyncSender<Event>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Runtime {
    pub fn start(shared: Arc<RuntimeShared>) -> Self {
        let (tx, rx) = mpsc::sync_channel(8192);
        let sender = tx.clone();
        let (done, ready) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("mixless-controllers".into())
            .spawn(move || {
                let mut w = Worker::new(shared, sender);
                w.scan();
                let _ = done.send(());
                loop {
                    match rx.recv_timeout(Duration::from_millis(2)) {
                        Ok(Event::Stop) => break,
                        Ok(Event::Wake) => {
                            w.next_scan = Instant::now();
                            w.reload();
                        }
                        Ok(Event::Packet(p, generation)) => {
                            if w.midi
                                .get(&p.device_id)
                                .is_some_and(|c| c.generation == generation)
                            {
                                w.receive(p);
                            }
                        }
                        Ok(Event::Feature(id, report, length, reply)) => {
                            let result = w
                                .hid
                                .get(&id)
                                .ok_or("HID device is not connected".to_string())
                                .and_then(|c| {
                                    let mut data = vec![0; length];
                                    data[0] = report;
                                    c.device
                                        .get_feature_report(&mut data)
                                        .map_err(|e| e.to_string())
                                        .map(|n| {
                                            data.truncate(n);
                                            data
                                        })
                                });
                            let _ = reply.send(result);
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if w.overflow.swap(false, Ordering::AcqRel) {
                        for (id, a) in w.router.release_all() {
                            w.deliver(id, Instant::now(), a);
                        }
                    }
                    if Instant::now() >= w.next_scan {
                        w.scan();
                    }
                    w.poll_hid();
                    let learning = {
                        let input = w.shared.input.lock().unwrap();
                        input.learning.then(|| input.learn_device.clone())
                    };
                    if learning != w.learning {
                        if let Some(scope) = &learning {
                            if let Some(id) = scope {
                                w.release(id);
                            } else {
                                for (id, a) in w.router.release_all() {
                                    w.deliver(id, Instant::now(), a);
                                }
                            }
                        }
                        w.learning = learning;
                    }
                    if Instant::now() >= w.next_feedback {
                        w.feedback();
                        w.next_feedback = Instant::now() + Duration::from_millis(33);
                    }
                }
                w.close_all();
            })
            .expect("controller worker");
        let _ = ready.recv_timeout(Duration::from_secs(2));
        Self {
            tx,
            thread: Some(handle),
        }
    }
    pub fn read_feature(&self, id: String, report: u8, length: usize) -> Result<Vec<u8>, String> {
        if !(1..=4096).contains(&length) {
            return Err("Feature report length must be 1–4096".into());
        }
        let (tx, rx) = mpsc::channel();
        self.tx
            .try_send(Event::Feature(id, report, length, tx))
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?
    }
    pub fn wake(&self) {
        let _ = self.tx.try_send(Event::Wake);
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if self
            .thread
            .as_ref()
            .is_some_and(|t| t.thread().id() == thread::current().id())
        {
            let tx = self.tx.clone();
            thread::spawn(move || {
                let _ = tx.send(Event::Stop);
            });
        } else {
            let _ = self.tx.send(Event::Stop);
        }
        if let Some(t) = self.thread.take() {
            if t.thread().id() == thread::current().id() {
                return;
            }
            let _ = t.join();
        }
    }
}
struct MidiConnection {
    _input: MidiInputConnection<()>,
    output: Option<MidiOutputConnection>,
    output_id: Option<String>,
    generation: u64,
}
struct HidConnection {
    device: hidapi::HidDevice,
    numbered: bool,
}
struct Worker {
    shared: Arc<RuntimeShared>,
    tx: SyncSender<Event>,
    overflow: Arc<AtomicBool>,
    midi: HashMap<String, MidiConnection>,
    hid: HashMap<String, HidConnection>,
    profiles: Vec<(DeviceAssignment, ControllerPreset)>,
    wildcard: ControllerPreset,
    router: Router,
    revision: u64,
    config: MidiConfig,
    next_scan: Instant,
    next_feedback: Instant,
    feedback_cache: HashMap<(String, usize), Vec<u8>>,
    learning: Option<Option<String>>,
}
impl Worker {
    fn new(shared: Arc<RuntimeShared>, tx: SyncSender<Event>) -> Self {
        Self {
            shared,
            tx,
            overflow: Arc::new(AtomicBool::new(false)),
            midi: HashMap::new(),
            hid: HashMap::new(),
            profiles: vec![],
            wildcard: ControllerPreset::blank("Legacy wildcard"),
            router: Router::default(),
            revision: 0,
            config: MidiConfig::default(),
            next_scan: Instant::now(),
            next_feedback: Instant::now(),
            feedback_cache: HashMap::new(),
            learning: None,
        }
    }
    fn snapshot(&self) -> mixless_protocol::EngineSnapshot {
        self.shared
            .snapshot
            .lock()
            .unwrap()
            .clone()
            .map_or_else(Default::default, |f| f())
    }
    fn deliver(&self, id: String, at: Instant, action: MidiAction) {
        let event = ControlEvent {
            device_id: id,
            received_at: at,
            action: action.clone(),
        };
        let handler = self.shared.handler.lock().unwrap().clone();
        let handled = handler.is_some_and(|h| h(event));
        if !handled {
            let mut input = self.shared.input.lock().unwrap();
            if input.queued.len() < 8192 || action.value == MidiValue::Release {
                input.queued.push(action);
            }
            drop(input);
            if let Some(n) = self.shared.notify.lock().unwrap().clone() {
                n();
            }
        }
        if !handled {
            return;
        }
        let mut latency = self.shared.latencies.lock().unwrap();
        if latency.len() == 8192 {
            latency.drain(..4096);
        }
        latency.push(at.elapsed().as_micros().min(u64::MAX as u128) as u64);
    }
    fn reload(&mut self) {
        let revision = self.shared.revision.load(Ordering::Acquire);
        if revision == self.revision {
            return;
        }
        let (profiles, wildcard) = {
            let s = self.shared.store.lock().unwrap();
            (
                s.state
                    .assignments
                    .iter()
                    .filter_map(|a| s.preset(&a.preset_id).map(|p| (a.clone(), p)))
                    .collect::<Vec<_>>(),
                s.state.wildcard.iter().map(Binding::from_legacy).collect(),
            )
        };
        let changed: Vec<_> = self
            .profiles
            .iter()
            .filter(|old| !profiles.contains(old))
            .cloned()
            .collect();
        for (a, p) in changed {
            for packet in &p.shutdown {
                self.send(&a, packet);
            }
            for id in a.ports.values() {
                self.release(id);
            }
        }
        let added: Vec<_> = profiles
            .iter()
            .filter(|p| !self.profiles.contains(p))
            .cloned()
            .collect();
        self.profiles = profiles;
        self.wildcard.bindings = wildcard;
        self.revision = revision;
        for (a, p) in added {
            for packet in &p.initialize {
                self.send(&a, packet);
            }
            for id in a.ports.values() {
                self.feedback_cache.retain(|(d, _), _| d != id);
            }
        }
    }
    fn release(&mut self, id: &str) {
        let ids: Vec<String> = self
            .profiles
            .iter()
            .find(|(a, _)| a.ports.values().any(|v| v == id))
            .map(|(a, _)| a.ports.values().cloned().collect())
            .unwrap_or_else(|| vec![id.into()]);
        for port in ids {
            for a in self.router.release_device(&port) {
                self.deliver(port.clone(), Instant::now(), a);
            }
        }

        self.feedback_cache.retain(|(d, _), _| d != id);
    }
    fn close_all(&mut self) {
        for (a, p) in self.profiles.clone() {
            for packet in &p.shutdown {
                self.send(&a, packet);
            }
        }
        for (id, a) in self.router.release_all() {
            self.deliver(id, Instant::now(), a);
        }
        self.midi.clear();
        self.hid.clear();
    }
    fn scan(&mut self) {
        self.reload();
        let config = self.shared.config.lock().unwrap().clone();
        if self.config.enabled && !config.enabled {
            self.close_all();
        }
        self.config = config.clone();
        let mut devices = Vec::new();
        let mut seen = Vec::new();
        match mixless_midi::input_ports() {
            Ok(ports) => {
                for port in ports {
                    let id = port.id.clone();
                    seen.push(id.clone());
                    let enabled = config.enabled
                        && config
                            .input_ids
                            .as_ref()
                            .is_none_or(|ids| ids.contains(&id));
                    if !enabled {
                        if self.midi.remove(&id).is_some() {
                            self.release(&id);
                        }
                    }
                    if enabled && self.midi.contains_key(&id) {
                        let desired = self.desired_output(&port);
                        if self.midi.get(&id).is_some_and(|c| {
                            c.output_id != desired || c.output.is_none() && desired.is_some()
                        }) {
                            let (output_id, output) = self.open_output(&port);
                            let c = self.midi.get_mut(&id).unwrap();
                            c.output_id = output_id;
                            c.output = output;
                            self.initialize(&id);
                        }
                    }
                    let state = if enabled {
                        if self.midi.contains_key(&id) {
                            ConnectionState::Connected
                        } else {
                            match self.open_midi(&port) {
                                Ok(c) => {
                                    self.midi.insert(id.clone(), c);
                                    self.initialize(&id);
                                    ConnectionState::Connected
                                }
                                Err(e) => ConnectionState::Error(e),
                            }
                        }
                    } else {
                        ConnectionState::Discovered
                    };
                    devices.push(ControllerDevice {
                        id,
                        name: port.name,
                        hid: false,
                        state,
                        vendor_id: None,
                        product_id: None,
                        interface: None,
                        usage_page: None,
                        usage: None,
                    });
                }
            }
            Err(e) => {
                devices.push(ControllerDevice {
                    id: "midi-error".into(),
                    name: "MIDI service".into(),
                    hid: false,
                    state: ConnectionState::Error(e),
                    vendor_id: None,
                    product_id: None,
                    interface: None,
                    usage_page: None,
                    usage: None,
                });
            }
        }
        if let Ok(inventory) = crate::hid_backend::devices() {
            for info in inventory {
                let identity = info
                    .serial_number()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| info.path().to_string_lossy().into_owned());
                let id = format!(
                    "hid:{:04x}:{:04x}:{}:{}:{}:{}",
                    info.vendor_id(),
                    info.product_id(),
                    identity,
                    info.interface_number(),
                    info.usage_page(),
                    info.usage()
                );
                seen.push(id.clone());
                let mut d = ControllerDevice {
                    id: id.clone(),
                    name: info.product_string().unwrap_or("HID device").into(),
                    hid: true,
                    state: ConnectionState::Discovered,
                    vendor_id: Some(info.vendor_id()),
                    product_id: Some(info.product_id()),
                    interface: Some(info.interface_number()),
                    usage_page: Some(info.usage_page()),
                    usage: Some(info.usage()),
                };
                let assigned = self.profiles.iter().any(|(a, p)| {
                    p.endpoints
                        .iter()
                        .any(|e| a.ports.get(&e.id) == Some(&id) && d.compatible(&e.matcher))
                });
                if config.enabled && assigned {
                    if !self.hid.contains_key(&id) {
                        match crate::hid_backend::open(info.path()) {
                            Ok(h) => {
                                let mut descriptor = [0u8; 4096];
                                let numbered = h
                                    .get_report_descriptor(&mut descriptor)
                                    .map(|n| has_report_id(&descriptor[..n]))
                                    .unwrap_or(true);
                                self.hid.insert(
                                    id.clone(),
                                    HidConnection {
                                        device: h,
                                        numbered,
                                    },
                                );
                                self.initialize(&id);
                            }
                            Err(e) => d.state = ConnectionState::Error(e.to_string()),
                        }
                    }
                    if self.hid.contains_key(&id) {
                        d.state = ConnectionState::Connected;
                    }
                } else if self.hid.remove(&id).is_some() {
                    self.release(&id);
                }
                devices.push(d);
            }
        }
        let gone: Vec<_> = self
            .midi
            .keys()
            .chain(self.hid.keys())
            .filter(|id| !seen.contains(id))
            .cloned()
            .collect();
        for id in gone {
            self.release(&id);
            self.midi.remove(&id);
            self.hid.remove(&id);
        }
        let old = self.shared.devices.lock().unwrap().clone();
        for d in old {
            if !seen.contains(&d.id)
                && self
                    .profiles
                    .iter()
                    .any(|(a, _)| a.ports.values().any(|v| v == &d.id))
            {
                devices.push(ControllerDevice {
                    state: ConnectionState::Offline,
                    ..d
                });
            }
        }
        for (a, p) in &self.profiles {
            for (endpoint, id) in &a.ports {
                if devices.iter().any(|d| &d.id == id) {
                    continue;
                }
                if let Some(e) = p.endpoints.iter().find(|e| &e.id == endpoint) {
                    let (hid, vendor_id, product_id, interface, usage_page, usage) = match e.matcher
                    {
                        DeviceMatcher::Midi { .. } => (false, None, None, None, None, None),
                        DeviceMatcher::Hid {
                            vendor_id,
                            product_id,
                            interface,
                            usage_page,
                            usage,
                        } => (
                            true,
                            Some(vendor_id),
                            Some(product_id),
                            interface,
                            usage_page,
                            usage,
                        ),
                    };
                    devices.push(ControllerDevice {
                        id: id.clone(),
                        name: format!("{} · {}", p.name, endpoint),
                        hid,
                        state: ConnectionState::Offline,
                        vendor_id,
                        product_id,
                        interface,
                        usage_page,
                        usage,
                    });
                }
            }
        }
        *self.shared.devices.lock().unwrap() = devices;
        self.next_scan = Instant::now() + Duration::from_millis(500);
    }
    fn open_midi(&self, port: &MidiPort) -> Result<MidiConnection, String> {
        let mut input = MidiInput::new("mixless-controller").map_err(|e| e.to_string())?;
        input.ignore(Ignore::Time);
        let p = input
            .ports()
            .into_iter()
            .find(|p| p.id() == port.id)
            .ok_or("Input disappeared")?;
        static GENERATION: AtomicU64 = AtomicU64::new(1);
        let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        let id = port.id.clone();
        let tx = self.tx.clone();
        let overflow = self.overflow.clone();
        let connection = input
            .connect(
                &p,
                "mixless-input",
                move |_, bytes, _| {
                    if bytes.len() > 4096 {
                        return;
                    }
                    let packet = ReceivedPacket {
                        device_id: id.clone(),
                        hid: false,
                        data: bytes.to_vec(),
                        received_at: Instant::now(),
                    };
                    if tx.try_send(Event::Packet(packet, generation)).is_err() {
                        overflow.store(true, Ordering::Release);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        let (output_id, output) = self.open_output(port);
        Ok(MidiConnection {
            _input: connection,
            output,
            output_id,
            generation,
        })
    }
    fn desired_output(&self, port: &MidiPort) -> Option<String> {
        let explicit = self
            .profiles
            .iter()
            .find_map(|(a, _)| {
                a.ports
                    .iter()
                    .find(|(_, id)| *id == &port.id)
                    .and_then(|(endpoint, _)| a.outputs.get(endpoint))
            })
            .cloned();
        let outputs = mixless_midi::output_ports().ok()?;
        let candidates: Vec<_> = outputs
            .into_iter()
            .filter(|p| {
                explicit
                    .as_ref()
                    .map_or_else(|| p.name == port.name, |id| p.id == *id)
            })
            .collect();
        (candidates.len() == 1).then(|| candidates[0].id.clone())
    }
    fn open_output(&self, port: &MidiPort) -> (Option<String>, Option<MidiOutputConnection>) {
        let id = self.desired_output(port);
        let connection = id.as_ref().and_then(|id| {
            let output = MidiOutput::new("mixless-feedback").ok()?;
            let port = output.ports().into_iter().find(|p| p.id() == *id)?;
            output.connect(&port, "mixless-output").ok()
        });
        (id, connection)
    }
    fn initialize(&mut self, id: &str) {
        for (a, p) in self.profiles.clone() {
            if a.ports.values().any(|v| v == id) {
                for packet in &p.initialize {
                    if a.ports.get(&packet.endpoint).is_some_and(|v| v == id) {
                        self.send(&a, packet);
                    }
                }
            }
        }
        self.feedback_cache.retain(|(d, _), _| d != id);
    }
    fn receive(&mut self, packet: ReceivedPacket) {
        *self.shared.last.lock().unwrap() = Some(packet.clone());
        {
            let mut input = self.shared.input.lock().unwrap();
            if !packet.hid
                && (!input.learning
                    || input
                        .learn_device
                        .as_ref()
                        .is_none_or(|id| id == &packet.device_id))
            {
                input.receive(&[], &packet.device_id, &packet.data);
            }
            if input.learning
                && input
                    .learn_device
                    .as_ref()
                    .is_none_or(|id| id == &packet.device_id)
            {
                return;
            }
        }
        let snapshot = self.snapshot();
        let mut actions = Vec::new();
        let mut assigned = false;
        for (a, p) in &self.profiles {
            for (endpoint, id) in &a.ports {
                if id == &packet.device_id {
                    assigned |= p.bindings.iter().any(|b| {
                        b.endpoint == *endpoint
                            && match &b.source {
                                InputSource::Midi {
                                    address: MidiAddress::Note { channel, number },
                                } => {
                                    packet.data.len() == 3
                                        && packet.data[0] & 15 == *channel
                                        && packet.data[1] == *number
                                        && matches!(packet.data[0] & 0xf0, 0x80 | 0x90)
                                }
                                InputSource::Midi {
                                    address: MidiAddress::Cc { channel, number },
                                } => {
                                    packet.data.len() == 3
                                        && packet.data[0] == 0xb0 | *channel
                                        && packet.data[1] == *number
                                }
                                InputSource::Midi {
                                    address: MidiAddress::Cc14 { channel, msb, lsb },
                                } => {
                                    packet.data.len() == 3
                                        && packet.data[0] == (0xb0 | *channel)
                                        && (packet.data[1] == *msb || packet.data[1] == *lsb)
                                }
                                InputSource::Midi {
                                    address: MidiAddress::PitchBend { channel },
                                } => packet.data.first() == Some(&(0xe0 | *channel)),
                                InputSource::Midi {
                                    address:
                                        MidiAddress::Rpn { channel, .. }
                                        | MidiAddress::Nrpn { channel, .. },
                                } => {
                                    packet.data.len() == 3
                                        && packet.data[0] == (0xb0 | *channel)
                                        && matches!(packet.data[1], 6 | 38 | 98..=101)
                                }
                                InputSource::Midi {
                                    address: MidiAddress::Sysex { prefix, .. },
                                } => packet.data.starts_with(prefix),
                                _ => false,
                            }
                    });
                    actions.extend(self.router.route_for_controller(
                        &a.device_id,
                        id,
                        endpoint,
                        p,
                        packet.hid,
                        &packet.data,
                        &snapshot,
                    ));
                }
            }
        }
        // Legacy wildcard is an explicit migration feature, never an automatic preset fallback.
        if !assigned && !packet.hid {
            actions.extend(self.router.route(
                &packet.device_id,
                "main",
                &self.wildcard,
                false,
                &packet.data,
                &snapshot,
            ));
        }
        for action in actions {
            self.deliver(packet.device_id.clone(), packet.received_at, action);
        }
    }
    fn poll_hid(&mut self) {
        let mut packets = Vec::new();
        let mut failed = Vec::new();
        let mut buffer = [0u8; 4096];
        for (id, c) in &self.hid {
            for _ in 0..64 {
                match c.device.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut data = Vec::with_capacity(n + 1);
                        if !c.numbered {
                            data.push(0);
                        }
                        data.extend_from_slice(&buffer[..n]);
                        packets.push(ReceivedPacket {
                            device_id: id.clone(),
                            hid: true,
                            data,
                            received_at: Instant::now(),
                        });
                    }
                    Err(_) => {
                        failed.push(id.clone());
                        break;
                    }
                }
            }
        }
        for id in failed {
            self.hid.remove(&id);
            self.release(&id);
            self.next_scan = Instant::now();
        }
        for packet in packets {
            self.receive(packet);
        }
    }
    fn send(&mut self, a: &DeviceAssignment, packet: &OutputPacket) -> bool {
        let Some(id) = a.ports.get(&packet.endpoint) else {
            return false;
        };
        let result = if let Some(c) = self.midi.get_mut(id) {
            match c.output.as_mut() {
                Some(output) => output.send(&packet.data).map_err(|e| e.to_string()),
                None => Err("No unique feedback output; assign an output in Endpoints".into()),
            }
        } else if let Some(c) = self.hid.get(id) {
            match packet.report {
                Some(ReportKind::Feature) => c
                    .device
                    .send_feature_report(&packet.data)
                    .map_err(|e| e.to_string()),
                Some(ReportKind::Output) => c
                    .device
                    .write(&packet.data)
                    .map_err(|e| e.to_string())
                    .and_then(|n| {
                        if n == packet.data.len() {
                            Ok(())
                        } else {
                            Err("Incomplete HID output report".into())
                        }
                    }),
                None => Err("Missing HID report kind".into()),
            }
        } else {
            return false;
        };
        let mut errors = self.shared.output_errors.lock().unwrap();
        match result {
            Ok(()) => {
                errors.remove(id);
                true
            }
            Err(e) => {
                errors.insert(id.clone(), e);
                false
            }
        }
    }

    fn feedback(&mut self) {
        let s = self.snapshot();
        for (a, p) in self.profiles.clone() {
            for (index, f) in p.feedback.iter().enumerate() {
                let value = match &f.signal {
                    FeedbackSignal::Control { target } => target_value(target, &s),
                    FeedbackSignal::Meter { deck } => deck
                        .map_or(s.master_level, |d| s.deck(d).level)
                        .into_iter()
                        .fold(0f32, f32::max)
                        .clamp(0., 1.) as f64,
                };
                let mut packet = f.packet.clone();
                let value = (f.off as f64 + (f.on as f64 - f.off as f64) * value).round() as u32;
                if !write_report_field(
                    &mut packet.data,
                    f.bit_offset,
                    f.bits,
                    f.little_endian,
                    value,
                ) {
                    continue;
                }
                let Some(id) = a.ports.get(&packet.endpoint) else {
                    continue;
                };
                let key = (id.clone(), index);
                if self.feedback_cache.get(&key) == Some(&packet.data) {
                    continue;
                }
                if self.send(&a, &packet) {
                    self.feedback_cache.insert(key, packet.data);
                }
            }
        }
    }
}
fn has_report_id(descriptor: &[u8]) -> bool {
    let mut i = 0;
    while i < descriptor.len() {
        let b = descriptor[i];
        if b == 0xfe {
            if i + 2 >= descriptor.len() {
                return false;
            }
            i += 3 + descriptor[i + 1] as usize;
            continue;
        }
        let size = match b & 3 {
            3 => 4,
            n => n as usize,
        };
        if b & 0xfc == 0x84 {
            return true;
        }
        i += 1 + size;
    }
    false
}
