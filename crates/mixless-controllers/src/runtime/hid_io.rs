//! Potentially blocking HID calls belong to their device thread, never the shared dispatcher.
use super::*;

#[derive(Clone)]
pub(super) struct OutputContext {
    pub adapter: Option<(String, u64)>,
    pub cancelled: Arc<AtomicBool>,
    pub shutdown: bool,
}
impl OutputContext {
    pub fn cleanup() -> Self {
        Self {
            adapter: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            shutdown: true,
        }
    }
    pub fn active(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
    }
}
pub(super) enum HidRequest {
    Write(OutputPacket, OutputContext),
    Feature {
        report: u8,
        length: usize,
        reply: FeatureReply,
        lease: Option<Arc<AtomicBool>>,
    },
}
pub(super) enum FeatureReply {
    Manual(mpsc::Sender<Result<Vec<u8>, String>>),
    Adapter {
        id: String,
        generation: u64,
        endpoint: String,
    },
}
pub(super) enum HidResult {
    Write {
        context: OutputContext,
        endpoint: String,
        result: Result<(), String>,
    },
    Feature {
        id: String,
        generation: u64,
        endpoint: String,
        result: Result<Vec<u8>, String>,
    },
    Disconnected(String),
}
pub(super) struct HidConnection {
    pub generation: u64,
    tx: SyncSender<HidRequest>,
    stop: Arc<AtomicBool>,
}
impl Drop for HidConnection {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl HidConnection {
    #[cfg(test)]
    pub(super) fn test_queue() -> (Self, mpsc::Receiver<HidRequest>) {
        let (tx, rx) = mpsc::sync_channel(256);
        (
            Self {
                generation: 1,
                tx,
                stop: Arc::new(AtomicBool::new(false)),
            },
            rx,
        )
    }

    pub fn start(
        id: String,
        device: hidapi::HidDevice,
        numbered: bool,
        events: SyncSender<Event>,
        overflow: Arc<AtomicBool>,
    ) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let generation = NEXT.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::sync_channel::<HidRequest>(256);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        thread::Builder::new()
            .name("mixless-hid-io".into())
            .spawn(move || {
                let outcome = |result| {
                    if events
                        .try_send(Event::HidResult(id.clone(), generation, result))
                        .is_err()
                    {
                        overflow.store(true, Ordering::Release);
                    }
                };
                let mut buffer = [0; 4096];
                while !stopped.load(Ordering::Acquire) {
                    match rx.recv_timeout(Duration::from_millis(1)) {
                        Ok(HidRequest::Write(packet, context)) => {
                            if stopped.load(Ordering::Acquire) || !context.active() {
                                continue;
                            }
                            let result = match packet.report {
                                Some(ReportKind::Feature) => device
                                    .send_feature_report(&packet.data)
                                    .map_err(|e| e.to_string()),
                                Some(ReportKind::Output) => device
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
                            };
                            outcome(HidResult::Write {
                                context,
                                endpoint: packet.endpoint,
                                result,
                            });
                        }
                        Ok(HidRequest::Feature {
                            report,
                            length,
                            reply,
                            lease,
                        }) => {
                            if stopped.load(Ordering::Acquire)
                                || lease.is_some_and(|stop| stop.load(Ordering::Acquire))
                            {
                                continue;
                            }
                            let mut data = vec![0; length];
                            data[0] = report;
                            let result = device
                                .get_feature_report(&mut data)
                                .map_err(|e| e.to_string())
                                .map(|n| {
                                    data.truncate(n);
                                    data
                                });
                            match reply {
                                FeatureReply::Manual(reply) => {
                                    let _ = reply.send(result);
                                }
                                FeatureReply::Adapter {
                                    id,
                                    generation,
                                    endpoint,
                                } => outcome(HidResult::Feature {
                                    id,
                                    generation,
                                    endpoint,
                                    result,
                                }),
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => (),
                    }
                    for _ in 0..64 {
                        let length = if numbered { 4096 } else { 4095 };
                        match device.read(&mut buffer[..length]) {
                            Ok(0) => break,
                            Ok(n) => {
                                let mut data = Vec::with_capacity(n + 1);
                                if !numbered {
                                    data.push(0);
                                }
                                data.extend_from_slice(&buffer[..n]);
                                let packet = ReceivedPacket {
                                    device_id: id.clone(),
                                    hid: true,
                                    data,
                                    received_at: Instant::now(),
                                };
                                if events
                                    .try_send(Event::HidPacket(packet, generation))
                                    .is_err()
                                {
                                    overflow.store(true, Ordering::Release);
                                }
                            }
                            Err(e) => {
                                stopped.store(true, Ordering::Release);
                                outcome(HidResult::Disconnected(e.to_string()));
                                return;
                            }
                        }
                    }
                }
                stopped.store(true, Ordering::Release);
            })
            .expect("HID I/O worker");
        Self {
            generation,
            tx,
            stop,
        }
    }
    pub fn alive(&self) -> bool {
        !self.stop.load(Ordering::Acquire)
    }
    pub fn send(&self, request: HidRequest) -> Result<(), String> {
        self.tx
            .try_send(request)
            .map_err(|_| "HID I/O queue is full or unavailable".into())
    }
}

impl Worker {
    pub(super) fn hid_result(&mut self, device: String, generation: u64, result: HidResult) {
        if !self
            .hid
            .get(&device)
            .is_some_and(|c| c.generation == generation)
        {
            return;
        }
        match result {
            HidResult::Disconnected(error) => {
                self.hid.remove(&device);
                self.release(&device);
                self.shared
                    .output_errors
                    .lock()
                    .unwrap()
                    .insert(device, error);
                self.next_scan = Instant::now();
            }
            HidResult::Write {
                context,
                endpoint,
                result,
            } => {
                if !context.active() || context.shutdown {
                    return;
                }
                match result {
                    Ok(()) => {
                        self.shared.output_errors.lock().unwrap().remove(&device);
                    }
                    Err(error) => {
                        self.shared
                            .output_errors
                            .lock()
                            .unwrap()
                            .insert(device.clone(), error);
                        self.feedback_cache.retain(|(id, _), _| id != &device);
                        if let Some((id, generation)) = context.adapter {
                            if self
                                .adapters
                                .get(&id)
                                .is_some_and(|w| w.generation == generation)
                            {
                                self.adapter_event(
                                    &id,
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
            HidResult::Feature {
                id,
                generation,
                endpoint,
                result,
            } => {
                if !self
                    .adapters
                    .get(&id)
                    .is_some_and(|w| w.generation == generation)
                {
                    return;
                }
                match result {
                    Ok(bytes) => self.adapter_event(
                        &id,
                        EventKind::Feature,
                        &endpoint,
                        &bytes,
                        Instant::now(),
                    ),
                    Err(_) => {
                        self.adapter_event(&id, EventKind::IoError, &endpoint, &[], Instant::now())
                    }
                }
            }
        }
    }
}
