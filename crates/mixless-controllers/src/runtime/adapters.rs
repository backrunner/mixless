//! A bounded interpreter queue per assigned controller; neither compilation nor guest code runs on
//! the transport dispatcher, GPUI thread or audio callback.
use super::*;
use crate::adapter::{AdapterEffect, AdapterEvent, AdapterInstance, AdapterState, EventKind};

pub(super) struct AdapterReply {
    pub id: String,
    pub generation: u64,
    pub at: Instant,
    pub result: Result<Vec<AdapterEffect>, String>,
}
struct Request {
    event: AdapterEvent,
    state: AdapterState,
    at: Instant,
}
pub(super) struct AdapterWorker {
    pub generation: u64,
    pub timer: Option<Instant>,
    pub last_state: Option<AdapterState>,
    pending: usize,
    pub fault: Arc<Mutex<Option<String>>>,
    tx: SyncSender<Request>,
    pub(super) stop: Arc<AtomicBool>,
}
impl Drop for AdapterWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl AdapterWorker {
    fn start(id: String, preset: ControllerPreset, sender: SyncSender<Event>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let generation = NEXT.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::sync_channel::<Request>(256);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let fault: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let failure = fault.clone();
        thread::Builder::new()
            .name("mixless-hid-adapter".into())
            .spawn(move || {
                let mut instance = match AdapterInstance::new(&preset) {
                    Ok(a) => a,
                    Err(e) => {
                        *failure.lock().unwrap() = Some(e.to_string());
                        return;
                    }
                };
                while !stopped.load(Ordering::Acquire) {
                    let request = match rx.recv_timeout(Duration::from_millis(20)) {
                        Ok(r) => r,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    let result = instance
                        .process(&request.event, &request.state)
                        .map_err(|e| e.to_string());
                    let failed = result.is_err();
                    if sender
                        .try_send(Event::Adapter(AdapterReply {
                            id: id.clone(),
                            generation,
                            at: request.at,
                            result,
                        }))
                        .is_err()
                    {
                        *failure.lock().unwrap() = Some("Adapter output queue overflow".into());
                        return;
                    }
                    if failed {
                        return;
                    }
                }
                // Disconnect is advisory; the handle may already have disappeared. Its effects cannot
                // reach a replacement assignment. Held actions are released by the host independently.
                let state = AdapterState::from_snapshot(
                    preset.adapter.as_ref().unwrap(),
                    &Default::default(),
                );
                let _ = instance.process(
                    &AdapterEvent {
                        kind: EventKind::Disconnect,
                        endpoint: String::new(),
                        data: vec![],
                        at_ms: i64::MAX as u64,
                    },
                    &state,
                );
            })
            .expect("HID adapter worker");
        Self {
            generation,
            timer: None,
            last_state: None,
            pending: 0,
            tx,
            stop,
            fault,
        }
    }
    fn enqueue(&mut self, request: Request) {
        if self.fault.lock().unwrap().is_some() {
            return;
        }
        if self.tx.try_send(request).is_err() {
            *self.fault.lock().unwrap() = Some("Adapter input queue overflow".into());
        } else {
            self.pending += 1;
        }
    }
}

impl Worker {
    pub(super) fn sync_adapters(&mut self) {
        let ready: Vec<_> = self
            .profiles
            .iter()
            .filter(|(a, p)| {
                p.adapter.is_some() && a.ports.values().all(|id| self.hid.contains_key(id))
            })
            .filter(|(a, _)| !self.adapters.contains_key(&a.device_id))
            .cloned()
            .collect();
        for (a, p) in ready {
            if self.adapters.len() >= 16 {
                self.shared.adapter_errors.lock().unwrap().insert(
                    a.device_id.clone(),
                    "At most 16 controller adapters can run at once".into(),
                );
                continue;
            }
            self.shared
                .adapter_errors
                .lock()
                .unwrap()
                .remove(&a.device_id);
            self.adapters.insert(
                a.device_id.clone(),
                AdapterWorker::start(a.device_id.clone(), p, self.tx.clone()),
            );
            self.adapter_event(&a.device_id, EventKind::Connect, "", &[], Instant::now());
        }
    }
    pub(super) fn adapter_event(
        &mut self,
        id: &str,
        kind: EventKind,
        endpoint: &str,
        data: &[u8],
        at: Instant,
    ) {
        let Some((_, p)) = self.profiles.iter().find(|(a, _)| a.device_id == id) else {
            return;
        };
        let Some(definition) = p.adapter.as_ref() else {
            return;
        };
        let snapshot = self.snapshot();
        let state = AdapterState::from_snapshot(definition, &snapshot);
        let Some(worker) = self.adapters.get_mut(id) else {
            return;
        };
        if kind == EventKind::State && worker.last_state.as_ref() == Some(&state) {
            return;
        }
        if kind == EventKind::State {
            worker.last_state = Some(state.clone());
        }
        let event = AdapterEvent {
            kind,
            endpoint: endpoint.into(),
            data: data.to_vec(),
            at_ms: self.started.elapsed().as_millis() as u64,
        };
        worker.enqueue(Request {
            event: event.clone(),
            state,
            at,
        });
        self.record(id, event, snapshot);
    }
    pub(super) fn poll_adapters(&mut self) {
        let failed: Vec<_> = self
            .adapters
            .iter()
            .filter_map(|(id, w)| w.fault.lock().unwrap().clone().map(|e| (id.clone(), e)))
            .collect();
        for (id, e) in failed {
            if !self.shared.adapter_errors.lock().unwrap().contains_key(&id) {
                // Queue/worker failures are not reproducible input events. Do not publish
                // a complete trace after losing part of the session.
                self.truncate_recording(&id);
                self.fail_adapter(&id, e);
            }
        }
        let due: Vec<_> = self
            .adapters
            .iter()
            // Earlier callbacks may replace this timer. Accept their effects before
            // enqueueing Timer, preserving the event order used by offline replay.
            .filter(|(_, w)| w.pending == 0 && w.timer.is_some_and(|t| Instant::now() >= t))
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            self.adapters.get_mut(&id).unwrap().timer = None;
            self.adapter_event(&id, EventKind::Timer, "", &[], Instant::now());
        }
    }
    pub(super) fn fail_adapter(&mut self, id: &str, message: String) {
        if let Some(worker) = self.adapters.get_mut(id) {
            *worker.fault.lock().unwrap() = Some(message.clone());
            worker.timer = None;
            worker.stop.store(true, Ordering::Release);
        }
        self.shared
            .adapter_errors
            .lock()
            .unwrap()
            .insert(id.into(), message);
        self.release_adapter_holds(id);
    }
    pub(super) fn release_adapter_holds(&mut self, id: &str) {
        for target in self.adapter_held.remove(id).unwrap_or_default() {
            if self
                .adapter_held
                .values()
                .any(|held| held.contains(&target))
            {
                continue;
            }
            self.deliver(
                id.into(),
                Instant::now(),
                MidiAction {
                    target,
                    value: MidiValue::Release,
                },
            );
        }
    }
    pub(super) fn stop_adapter(&mut self, id: &str) {
        self.adapters.remove(id);
        self.release_adapter_holds(id);
        self.shared.adapter_errors.lock().unwrap().remove(id);
    }
    pub(super) fn adapter_reply(&mut self, reply: AdapterReply) {
        let Some(worker) = self.adapters.get_mut(&reply.id) else {
            return;
        };
        if worker.generation != reply.generation || worker.fault.lock().unwrap().is_some() {
            return;
        }
        worker.pending = worker.pending.saturating_sub(1);
        let effects = match reply.result {
            Ok(e) => e,
            Err(e) => {
                self.fail_adapter(&reply.id, e);
                return;
            }
        };
        let Some(assignment) = self
            .profiles
            .iter()
            .find(|(a, _)| a.device_id == reply.id)
            .map(|(a, _)| a.clone())
        else {
            return;
        };
        for effect in effects {
            match effect {
                AdapterEffect::Action { action } => {
                    let held = self.adapter_held.entry(reply.id.clone()).or_default();
                    if action.value == MidiValue::Hold && !held.contains(&action.target) {
                        held.push(action.target.clone());
                    }
                    if action.value == MidiValue::Release {
                        held.retain(|t| t != &action.target);
                    }
                    if action.value != MidiValue::Release
                        || !self
                            .adapter_held
                            .values()
                            .any(|h| h.contains(&action.target))
                    {
                        self.deliver(reply.id.clone(), reply.at, action);
                    }
                }
                AdapterEffect::Timer { delay_ms } => {
                    self.adapters.get_mut(&reply.id).unwrap().timer =
                        Some(Instant::now() + Duration::from_millis(delay_ms as u64));
                }
                AdapterEffect::Write { packet } => {
                    if !self.send(&assignment, &packet) {
                        self.adapter_event(
                            &reply.id,
                            EventKind::IoError,
                            &packet.endpoint,
                            &[],
                            Instant::now(),
                        );
                    }
                }
                AdapterEffect::ReadFeature {
                    endpoint,
                    report_id,
                    length,
                } => {
                    let result = assignment
                        .ports
                        .get(&endpoint)
                        .and_then(|id| self.hid.get(id))
                        .ok_or_else(|| "Device disconnected".to_string())
                        .and_then(|c| {
                            c.send(HidRequest::Feature {
                                report: report_id,
                                length: length as usize,
                                reply: FeatureReply::Adapter {
                                    id: reply.id.clone(),
                                    generation: reply.generation,
                                    endpoint: endpoint.clone(),
                                },
                                lease: self.adapters.get(&reply.id).map(|w| w.stop.clone()),
                            })
                        });
                    if result.is_err() {
                        self.adapter_event(
                            &reply.id,
                            EventKind::IoError,
                            &endpoint,
                            &[],
                            Instant::now(),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;
