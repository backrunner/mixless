//! Assignment and logical-controller session boundaries.
use super::*;

impl Worker {
    pub(super) fn restart(&mut self, id: &str) {
        self.reload();
        let ports: Vec<_> = self
            .profiles
            .iter()
            .find(|(a, _)| a.ports.values().any(|port| port == id))
            .map(|(a, _)| a.ports.values().cloned().collect())
            .unwrap_or_else(|| vec![id.into()]);
        self.release(id);
        // Reapplying is a logical controller restart, including secondary interfaces.
        for port in ports {
            self.initialize(&port);
        }
        self.sync_adapters();
    }
    pub(super) fn reload(&mut self) {
        let revision = self.shared.revision.load(Ordering::Acquire);
        if revision == self.revision {
            return;
        }
        let (profiles, wildcard, hid_matchers) = {
            let s = self.shared.store.lock().unwrap();
            (
                s.state
                    .assignments
                    .iter()
                    .filter_map(|a| s.preset(&a.preset_id).map(|p| (a.clone(), p)))
                    .collect::<Vec<_>>(),
                s.state.wildcard.iter().map(Binding::from_legacy).collect(),
                s.all()
                    .into_iter()
                    .flat_map(|p| p.endpoints)
                    .filter_map(|e| {
                        matches!(e.matcher, DeviceMatcher::Hid { .. }).then_some(e.matcher)
                    })
                    .collect(),
            )
        };
        let changed: Vec<_> = self
            .profiles
            .iter()
            .filter(|old| !profiles.contains(old))
            .cloned()
            .collect();
        for (a, p) in changed {
            self.retire_output(&a.device_id);
            self.stop_adapter(&a.device_id);
            for packet in &p.shutdown {
                self.send_packet(&a, packet, true);
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
        self.hid_matchers = hid_matchers;
        if self.wildcard.bindings != wildcard {
            self.wildcard_since = Instant::now();
            for (id, action) in self.router.release_preset(&self.wildcard.id) {
                self.deliver(id, Instant::now(), action);
            }
        }
        self.wildcard.bindings = wildcard;
        self.revision = revision;
        for (a, p) in added {
            let since = Instant::now();
            for id in a.ports.values() {
                self.input_since.insert(id.clone(), since);
            }
            if p.adapter.is_none()
                && a.ports
                    .values()
                    .all(|id| self.midi.contains_key(id) || self.hid.contains_key(id))
            {
                self.record(
                    &a.device_id,
                    AdapterEvent {
                        kind: EventKind::Connect,
                        endpoint: String::new(),
                        data: vec![],
                        at_ms: self.started.elapsed().as_millis() as u64,
                    },
                    self.snapshot(),
                );
            }
            for packet in &p.initialize {
                self.send(&a, packet);
            }
            for id in a.ports.values() {
                self.feedback_cache.retain(|(d, _), _| d != id);
            }
        }
    }
    pub(super) fn release(&mut self, id: &str) {
        self.release_session(id, true);
    }
    pub(super) fn release_for_learning(&mut self, id: &str) {
        // Release performance state without retiring packets captured after arm.
        self.release_session(id, false);
    }
    fn release_session(&mut self, id: &str, retire_input: bool) {
        if let Some(primary) = self
            .profiles
            .iter()
            .find(|(a, _)| a.ports.values().any(|v| v == id))
            .map(|(a, _)| a.device_id.clone())
        {
            self.retire_output(&primary);
            self.record(
                &primary,
                AdapterEvent {
                    kind: EventKind::Disconnect,
                    endpoint: String::new(),
                    data: vec![],
                    at_ms: self.started.elapsed().as_millis() as u64,
                },
                self.snapshot(),
            );
            self.stop_adapter(&primary);
        }
        let ids: Vec<String> = self
            .profiles
            .iter()
            .find(|(a, _)| a.ports.values().any(|v| v == id))
            .map(|(a, _)| a.ports.values().cloned().collect())
            .unwrap_or_else(|| vec![id.into()]);
        let since = Instant::now();
        for port in ids {
            if retire_input {
                self.input_since.insert(port.clone(), since);
                self.shared.input.lock().unwrap().forget_device(&port);
            }
            self.feedback_cache.retain(|(d, _), _| d != &port);
            for a in self.router.release_device(&port) {
                self.deliver(port.clone(), Instant::now(), a);
            }
        }
    }
    pub(super) fn close_all(&mut self) {
        for (a, p) in self.profiles.clone() {
            // Record the real session boundary before closing ports, including on disable.
            self.release(&a.device_id);
            for packet in &p.shutdown {
                self.send_packet(&a, packet, true);
            }
        }
        for (id, a) in self.router.release_all() {
            self.deliver(id, Instant::now(), a);
        }
        self.midi.clear();
        self.hid.clear();
        self.hid_open.clear();
    }
    pub(super) fn initialize(&mut self, id: &str) {
        for (a, p) in self.profiles.clone() {
            if a.ports.values().any(|v| v == id) {
                if p.adapter.is_none()
                    && a.ports
                        .values()
                        .all(|port| self.midi.contains_key(port) || self.hid.contains_key(port))
                {
                    self.record(
                        &a.device_id,
                        AdapterEvent {
                            kind: EventKind::Connect,
                            endpoint: String::new(),
                            data: vec![],
                            at_ms: self.started.elapsed().as_millis() as u64,
                        },
                        self.snapshot(),
                    );
                }
                for packet in &p.initialize {
                    if a.ports.get(&packet.endpoint).is_some_and(|v| v == id) {
                        self.send(&a, packet);
                    }
                }
            }
        }
        self.feedback_cache.retain(|(d, _), _| d != id);
    }
}
