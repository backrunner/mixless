//! Nonblocking discovery polling and explicit input/output port selection.
use super::*;

impl Worker {
    pub(super) fn scan(&mut self) {
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
        self.poll_hid_inventory();
        {
            for info in self.hid_inventory.clone() {
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
                if self.hid.get(&id).is_some_and(|c| !c.alive()) {
                    self.hid.remove(&id);
                    self.release(&id);
                }
                if config.enabled && assigned {
                    if !self.hid.contains_key(&id) {
                        if !self.hid_open.contains_key(&id) {
                            match crate::hid_backend::request_open(info.path()) {
                                Ok(pending) => {
                                    self.hid_open.insert(id.clone(), pending);
                                }
                                Err(e) => d.state = ConnectionState::Error(e),
                            }
                        }
                        let opened =
                            self.hid_open
                                .get(&id)
                                .and_then(|pending| match pending.try_recv() {
                                    Ok(result) => Some(result),
                                    Err(mpsc::TryRecvError::Empty) => None,
                                    Err(mpsc::TryRecvError::Disconnected) => {
                                        Some(Err("HID open worker unavailable".into()))
                                    }
                                });
                        if let Some(opened) = opened {
                            self.hid_open.remove(&id);
                            match opened {
                                Ok(h) => {
                                    self.hid.insert(
                                        id.clone(),
                                        HidConnection::start(
                                            id.clone(),
                                            h.device,
                                            h.numbered,
                                            self.tx.clone(),
                                            self.overflow.clone(),
                                        ),
                                    );
                                    self.initialize(&id);
                                }
                                Err(e) => d.state = ConnectionState::Error(e.to_string()),
                            }
                        }
                    }
                    if self.hid.contains_key(&id) {
                        d.state = ConnectionState::Connected;
                    }
                } else {
                    self.hid_open.remove(&id);
                    if self.hid.remove(&id).is_some() {
                        self.release(&id);
                    }
                }
                devices.push(d);
            }
        }
        self.hid_open.retain(|id, _| seen.contains(id));
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
                        name: format!("{} ({})", p.name, endpoint),
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
        self.sync_adapters();
        self.input_since.retain(|id, _| {
            seen.contains(id)
                || self
                    .profiles
                    .iter()
                    .any(|(a, _)| a.ports.values().any(|port| port == id))
        });
        *self.shared.devices.lock().unwrap() = devices;
        self.next_scan = Instant::now() + Duration::from_millis(500);
    }
    pub(super) fn poll_hid_inventory(&mut self) {
        if let Some(pending) = &self.hid_scan {
            match pending.try_recv() {
                Ok(result) => {
                    if let Ok(inventory) = result {
                        self.hid_inventory = inventory;
                    }
                    self.hid_scan = None;
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => self.hid_scan = None,
            }
        }
        self.hid_scan = crate::hid_backend::request_devices().ok();
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
}
