mod adapters;
mod discovery;
mod hid_io;
mod lifecycle;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod test_support;
use hid_io::{FeatureReply, HidConnection, HidRequest, HidResult, OutputContext};
pub(crate) mod recording;
use crate::adapter::{AdapterEvent, EventKind};
use crate::profile::PresetStore;
use crate::router::Router;
use crate::*;
use adapters::{AdapterReply, AdapterWorker};
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
    pub adapter_errors: Mutex<HashMap<String, String>>,
    pub recording: Mutex<Option<recording::Recorder>>,
}
enum Event {
    Adapter(AdapterReply),
    HidPacket(ReceivedPacket, u64),
    HidResult(String, u64, HidResult),
    Packet(ReceivedPacket, u64),
    Feature(String, u8, usize, mpsc::Sender<Result<Vec<u8>, String>>),
    Wake,
    Restart(String),
    StopRecording(mpsc::Sender<Result<crate::replay::ReportTrace, String>>),
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
                // Bootstrap before accepting control traffic. Subsequent discovery and open
                // requests are polled without waiting on the performance dispatcher.
                w.hid_inventory = crate::hid_backend::devices().unwrap_or_default();
                w.scan();
                let _ = done.send(());
                loop {
                    match rx.recv_timeout(Duration::from_millis(2)) {
                        Ok(Event::StopRecording(reply)) => {
                            let trace = w
                                .shared
                                .recording
                                .lock()
                                .unwrap()
                                .take()
                                .map(|r| r.trace)
                                .ok_or_else(|| "No controller capture is running".to_string());
                            let _ = reply.send(trace);
                        }
                        Ok(Event::Stop) => break,
                        Ok(Event::Adapter(reply)) => w.adapter_reply(reply),
                        Ok(Event::HidPacket(p, generation)) => {
                            if w.hid
                                .get(&p.device_id)
                                .is_some_and(|c| c.generation == generation)
                            {
                                w.receive(p);
                            }
                        }
                        Ok(Event::HidResult(id, generation, result)) => {
                            w.hid_result(id, generation, result)
                        }
                        Ok(Event::Restart(id)) => {
                            w.restart(&id);
                        }
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
                            if let Some(c) = w.hid.get(&id) {
                                let backup = reply.clone();
                                if let Err(e) = c.send(HidRequest::Feature {
                                    report,
                                    length,
                                    reply: FeatureReply::Manual(reply),
                                    lease: None,
                                }) {
                                    let _ = backup.send(Err(e));
                                }
                            } else {
                                let _ = reply.send(Err("HID device is not connected".into()));
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if w.overflow.swap(false, Ordering::AcqRel) {
                        w.recover_overflow();
                    }
                    if Instant::now() >= w.next_scan {
                        w.scan();
                    }
                    w.poll_adapters();
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
    pub fn stop_recording(&self) -> Result<crate::replay::ReportTrace, String> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .try_send(Event::StopRecording(tx))
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?
    }
    pub fn restart(&self, id: String) {
        let _ = self.tx.try_send(Event::Restart(id));
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
struct Worker {
    shared: Arc<RuntimeShared>,
    tx: SyncSender<Event>,
    overflow: Arc<AtomicBool>,
    discard_before: Option<Instant>,
    input_since: HashMap<String, Instant>,
    midi: HashMap<String, MidiConnection>,
    hid: HashMap<String, HidConnection>,
    hid_inventory: Vec<hidapi::DeviceInfo>,
    hid_scan: Option<crate::hid_backend::Pending<Vec<hidapi::DeviceInfo>>>,
    hid_open: HashMap<String, crate::hid_backend::Pending<crate::hid_backend::OpenedDevice>>,
    profiles: Vec<(DeviceAssignment, ControllerPreset)>,
    wildcard: ControllerPreset,
    wildcard_since: Instant,
    router: Router,
    revision: u64,
    config: MidiConfig,
    next_scan: Instant,
    next_feedback: Instant,
    feedback_cache: HashMap<(String, usize), Vec<u8>>,
    output_sessions: HashMap<String, Arc<AtomicBool>>,
    learning: Option<Option<String>>,
    started: Instant,
    adapters: HashMap<String, AdapterWorker>,
    adapter_held: HashMap<String, Vec<MidiTarget>>,
}
impl Worker {
    fn new(shared: Arc<RuntimeShared>, tx: SyncSender<Event>) -> Self {
        Self {
            shared,
            tx,
            overflow: Arc::new(AtomicBool::new(false)),
            discard_before: None,
            input_since: HashMap::new(),
            midi: HashMap::new(),
            hid: HashMap::new(),
            hid_inventory: vec![],
            hid_scan: None,
            hid_open: HashMap::new(),
            profiles: vec![],
            wildcard: ControllerPreset::blank("Legacy wildcard"),
            wildcard_since: Instant::now(),
            router: Router::default(),
            revision: 0,
            config: MidiConfig::default(),
            next_scan: Instant::now(),
            next_feedback: Instant::now(),
            feedback_cache: HashMap::new(),
            output_sessions: HashMap::new(),
            learning: None,
            started: Instant::now(),
            adapters: HashMap::new(),
            adapter_held: HashMap::new(),
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
    fn recover_overflow(&mut self) {
        // Invalidate packets already queued before the lost edge. Replaying an older
        // press after releasing holds could otherwise leave the control held forever.
        self.discard_before = Some(Instant::now());
        if let Some(recorder) = self.shared.recording.lock().unwrap().as_mut() {
            recorder.trace.truncated = true;
        }
        for id in self.adapters.keys().cloned().collect::<Vec<_>>() {
            self.fail_adapter(&id, "Controller input queue overflow".into());
        }
        for (id, action) in self.router.release_all() {
            self.deliver(id, Instant::now(), action);
        }
    }
    fn deliver(&self, id: String, at: Instant, action: MidiAction) {
        // Declarative routing and adapters share the same engine controls. Releasing one
        // source must not end a hold still owned by either kind of controller.
        if action.value == MidiValue::Release
            && (self.router.is_held(&action.target)
                || self
                    .adapter_held
                    .values()
                    .any(|held| held.iter().any(|t| t.holds_same_control(&action.target))))
        {
            return;
        }
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
    fn receive(&mut self, packet: ReceivedPacket) {
        if self
            .discard_before
            .is_some_and(|cutoff| packet.received_at < cutoff)
            || self
                .input_since
                .get(&packet.device_id)
                .is_some_and(|since| packet.received_at < *since)
        {
            return;
        }
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
        let mut adapter_inputs = Vec::new();
        let mut recorded = Vec::new();
        for (a, p) in &self.profiles {
            for (endpoint, id) in &a.ports {
                if id == &packet.device_id {
                    if p.adapter.is_some() {
                        adapter_inputs.push((a.device_id.clone(), endpoint.clone()));
                        assigned = true;
                        continue;
                    }
                    recorded.push((a.device_id.clone(), endpoint.clone()));
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
        for (id, endpoint) in adapter_inputs {
            self.adapter_event(
                &id,
                EventKind::Input,
                &endpoint,
                &packet.data,
                packet.received_at,
            );
        }
        for (id, endpoint) in recorded {
            self.record(
                &id,
                AdapterEvent {
                    kind: EventKind::Input,
                    endpoint,
                    data: packet.data.clone(),
                    at_ms: self.started.elapsed().as_millis() as u64,
                },
                snapshot.clone(),
            );
        }
        // Legacy wildcard is an explicit migration feature, never an automatic preset fallback.
        if !assigned && !packet.hid && packet.received_at >= self.wildcard_since {
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
    fn send(&mut self, a: &DeviceAssignment, packet: &OutputPacket) -> bool {
        self.send_packet(a, packet, false)
    }
    fn retire_output(&mut self, id: &str) {
        if let Some(cancelled) = self.output_sessions.remove(id) {
            cancelled.store(true, Ordering::Release);
        }
    }
    fn output_context(&mut self, id: &str, shutdown: bool) -> OutputContext {
        if shutdown {
            // Cleanup is ordered before the next initialization on the same device queue.
            // It is best effort and carries no authority over the replacement's error state.
            return OutputContext::cleanup();
        }
        let adapter = self.adapters.get(id);
        OutputContext {
            adapter: adapter.map(|w| (id.into(), w.generation)),
            cancelled: adapter.map_or_else(
                || self.output_sessions.entry(id.into()).or_default().clone(),
                |w| w.stop.clone(),
            ),
            shutdown: false,
        }
    }
    fn send_packet(&mut self, a: &DeviceAssignment, packet: &OutputPacket, shutdown: bool) -> bool {
        let Some(id) = a.ports.get(&packet.endpoint) else {
            return false;
        };
        let context = self.output_context(&a.device_id, shutdown);
        let synchronous = self.midi.contains_key(id);
        let result = if let Some(c) = self.midi.get_mut(id) {
            match c.output.as_mut() {
                Some(output) => output.send(&packet.data).map_err(|e| e.to_string()),
                None => Err("No unique feedback output; assign an output in Endpoints".into()),
            }
        } else if let Some(c) = self.hid.get(id) {
            c.send(HidRequest::Write(packet.clone(), context))
        } else {
            return false;
        };
        let mut errors = self.shared.output_errors.lock().unwrap();
        match result {
            Ok(()) => {
                // A successful enqueue is not successful HID I/O. Its completion clears errors.
                if synchronous && !shutdown {
                    errors.remove(id);
                }
                true
            }
            Err(e) => {
                if !shutdown {
                    errors.insert(id.clone(), e);
                }
                false
            }
        }
    }

    fn feedback(&mut self) {
        for id in self.adapters.keys().cloned().collect::<Vec<_>>() {
            self.adapter_event(&id, EventKind::State, "", &[], Instant::now());
        }
        let s = self.snapshot();
        for (a, p) in self
            .profiles
            .iter()
            .filter(|(_, p)| p.adapter.is_none())
            .cloned()
            .collect::<Vec<_>>()
        {
            self.record(
                &a.device_id,
                AdapterEvent {
                    kind: EventKind::State,
                    endpoint: String::new(),
                    data: vec![],
                    at_ms: self.started.elapsed().as_millis() as u64,
                },
                s.clone(),
            );
            for (index, f) in p.feedback.iter().enumerate() {
                let Some(packet) = crate::replay::feedback_packet(f, &s) else {
                    continue;
                };
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
