use super::Mode;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{self, SyncSender},
    Arc, Mutex,
};
#[cfg(target_os = "macos")]
#[path = "metal.rs"]
mod metal;
#[path = "metrics.rs"]
mod metrics;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

static ACTIVE: Mutex<Option<Arc<Observer>>> = Mutex::new(None);
const QUEUE: usize = 256;
const MAX_FRAMES: usize = 512;
const MAX_TRIALS: usize = 64;
const CHUNK: usize = 2048;

pub(crate) struct Input {
    pub frames: Vec<[f32; 40]>,
    pub tonality: u32,
}
struct Job {
    input: Arc<Input>,
    shapes: Vec<[f32; 16]>,
    cpu: Vec<Option<f32>>,
    groups: Vec<usize>,
}
#[derive(Default)]
struct Pending {
    shapes: Vec<[f32; 16]>,
    cpu: Vec<Option<f32>>,
    groups: Vec<usize>,
}
enum Message {
    Job(Job),
    Finish,
}
struct Observer {
    sender: SyncSender<Message>,
    submitted: AtomicU64,
    dropped: AtomicU64,
    skipped_inputs: AtomicU64,
    cpu_scores: AtomicU64,
    cpu_nanos: AtomicU64,
    cache_hits: AtomicU64,
    dropped_queries: AtomicU64,
    mode: Mode,
    queue_wait_nanos: AtomicU64,
}
pub(crate) struct Capture {
    input: Arc<Input>,
    observer: Arc<Observer>,
    pending: Mutex<Pending>,
}
impl Capture {
    pub(crate) fn scored(&self, elapsed: std::time::Duration) {
        self.observer.cpu_scores.fetch_add(1, Ordering::Relaxed);
        self.observer.cpu_nanos.fetch_add(
            elapsed.as_nanos().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
    }
    pub(crate) fn cache_hit(&self) {
        self.observer.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn new(build: impl FnOnce() -> Input) -> Option<Self> {
        let observer = ACTIVE.lock().unwrap().clone()?;
        let input = build();
        if input.frames.len() > MAX_FRAMES {
            observer.skipped_inputs.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        Some(Self {
            input: Arc::new(input),
            observer,
            pending: Mutex::new(Pending::default()),
        })
    }
    pub(crate) fn observe(&self, shapes: impl Iterator<Item = [f32; 16]>, cpu: &[Option<f32>]) {
        if cpu.is_empty() {
            return;
        }
        if cpu.len() > MAX_TRIALS {
            self.observer.dropped.fetch_add(1, Ordering::Relaxed);
            self.observer
                .dropped_queries
                .fetch_add(cpu.len() as u64, Ordering::Relaxed);
            return;
        }
        let mut pending = self.pending.lock().unwrap();
        if pending.cpu.len() + cpu.len() > CHUNK {
            self.flush(&mut pending);
        }
        pending.shapes.extend(shapes);
        pending.cpu.extend_from_slice(cpu);
        pending.groups.push(cpu.len());
        assert_eq!(pending.shapes.len(), pending.cpu.len());
    }
    fn flush(&self, pending: &mut Pending) {
        if pending.cpu.is_empty() {
            return;
        }
        let values = std::mem::take(pending);
        let count = values.cpu.len();
        let message = Message::Job(Job {
            input: Arc::clone(&self.input),
            shapes: values.shapes,
            cpu: values.cpu,
            groups: values.groups,
        });
        let started = std::time::Instant::now();
        let sent = match self.observer.mode {
            Mode::Complete => self.observer.sender.send(message).is_ok(),
            Mode::BestEffort => self.observer.sender.try_send(message).is_ok(),
        };
        self.observer.queue_wait_nanos.fetch_add(
            started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        if sent {
            self.observer.submitted.fetch_add(1, Ordering::Relaxed);
        } else {
            self.observer.dropped.fetch_add(1, Ordering::Relaxed);
            self.observer
                .dropped_queries
                .fetch_add(count as u64, Ordering::Relaxed);
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let mut pending = self.pending.lock().unwrap();
        self.flush(&mut pending);
    }
}

/// Run normal CPU planning and compare observed scoring batches asynchronously.
/// Capture is process-wide; concurrent audit sessions are not supported.
/// The closure's result is returned verbatim even if Metal is unavailable or
/// fails. Finish all planning jobs within the closure before returning.
/// Trace builds do not start GPU work. This is not a GPU selection backend.
pub fn compare<T>(mode: Mode, run: impl FnOnce() -> T) -> (T, Value) {
    if cfg!(feature = "trace") {
        return (run(), json!({"status":"disabled_for_trace"}));
    }
    #[cfg(not(target_os = "macos"))]
    return (run(), json!({"status":"unsupported_platform"}));
    #[cfg(target_os = "macos")]
    {
        let started = std::time::Instant::now();
        let mut active = ACTIVE.lock().unwrap();
        if active.is_some() {
            drop(active);
            return (run(), json!({"status":"already_active"}));
        }
        let (sender, receiver) = mpsc::sync_channel(QUEUE);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("mixless-gpu-audit".into())
            .spawn(move || {
                objc::rc::autoreleasepool(|| {
                    let engine = metal::Engine::new();
                    let _ = ready_tx.send(
                        engine
                            .as_ref()
                            .map(|e| e.name.clone())
                            .map_err(Clone::clone),
                    );
                    engine.map(|engine| metal::drain(engine, receiver))
                })
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                drop(active);
                return (
                    run(),
                    json!({"status":"unavailable", "error":error.to_string()}),
                );
            }
        };
        let device = match ready_rx.recv() {
            Ok(Ok(name)) => name,
            other => {
                let _ = worker.join();
                drop(active);
                return (
                    run(),
                    json!({"status":"unavailable", "error":format!("{other:?}")}),
                );
            }
        };
        let observer = Arc::new(Observer {
            sender,
            submitted: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            skipped_inputs: AtomicU64::new(0),
            cpu_scores: AtomicU64::new(0),
            cpu_nanos: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            dropped_queries: AtomicU64::new(0),
            mode,
            queue_wait_nanos: AtomicU64::new(0),
        });
        *active = Some(Arc::clone(&observer));
        drop(active);
        let mut session = Session {
            observer,
            worker: Some(worker),
        };
        let planning = std::time::Instant::now();
        let result = run();
        let planner_seconds = planning.elapsed().as_secs_f64();
        let outcome = session.finish();
        let submitted = session.observer.submitted.load(Ordering::Relaxed);
        let dropped = session.observer.dropped.load(Ordering::Relaxed);
        let skipped = session.observer.skipped_inputs.load(Ordering::Relaxed);
        let report = match outcome {
            Ok(metrics) => json!({
                "status": if dropped == 0 && skipped == 0 && metrics.uncompared == 0 && metrics.jobs == submitted && metrics.queries == session.observer.cpu_scores.load(Ordering::Relaxed) + session.observer.cache_hits.load(Ordering::Relaxed) {"complete"} else {"incomplete"},
                "mode":"shadow_only", "device":device, "submitted_jobs":submitted,
                "capture_mode":format!("{mode:?}"),
                "dropped_jobs":dropped, "skipped_inputs":skipped,
                "planner_seconds":planner_seconds, "total_seconds":started.elapsed().as_secs_f64(),
                "cpu_scores":session.observer.cpu_scores.load(Ordering::Relaxed),
                "cpu_score_seconds_sum":session.observer.cpu_nanos.load(Ordering::Relaxed) as f64 / 1e9,
                "cache_hits":session.observer.cache_hits.load(Ordering::Relaxed),
                "dropped_queries":session.observer.dropped_queries.load(Ordering::Relaxed),
                "queue_submit_seconds_sum":session.observer.queue_wait_nanos.load(Ordering::Relaxed) as f64 / 1e9,
                "comparison":metrics.json(),
            }),
            Err(error) => {
                json!({"status":"failed", "mode":"shadow_only", "error":error, "submitted_jobs":submitted, "dropped_jobs":dropped})
            }
        };
        (result, report)
    }
}

#[cfg(target_os = "macos")]
struct Session {
    observer: Arc<Observer>,
    worker: Option<std::thread::JoinHandle<Result<metrics::Metrics, String>>>,
}
#[cfg(target_os = "macos")]
impl Session {
    fn finish(&mut self) -> Result<metrics::Metrics, String> {
        // No GPU or file operations run while the global observer lock is held.
        *ACTIVE.lock().unwrap() = None;
        let _ = self.observer.sender.send(Message::Finish);
        self.worker
            .take()
            .ok_or("audit already finished")?
            .join()
            .map_err(|_| "GPU worker panicked")?
    }
}
#[cfg(target_os = "macos")]
impl Drop for Session {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.finish();
        }
    }
}
