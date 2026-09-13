//! Post-master stereo capture. The callback only tries a lock and pushes into
//! a preallocated ring; a separate thread owns the WAV and all filesystem I/O.
use std::{
    fs::OpenOptions,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};

#[derive(Clone, Debug, Default)]
pub struct RecordingStatus {
    pub active: bool,
    pub path: Option<PathBuf>,
    pub seconds: f64,
    pub dropped_frames: u64,
    pub error: Option<String>,
}

#[derive(Default)]
struct Progress {
    active: AtomicBool,
    frames: AtomicU64,
    dropped: AtomicU64,
    error: Mutex<Option<String>>,
}

pub(crate) struct RecordingSink {
    producer: rtrb::Producer<[f32; 2]>,
    progress: Arc<Progress>,
    sample_rate: u32,
}

impl RecordingSink {
    pub fn push(&mut self, sample: [f32; 2], sample_rate: u32) {
        if !self.progress.active.load(Ordering::Relaxed) {
            return;
        }
        if sample_rate != self.sample_rate || self.producer.push(sample).is_err() {
            self.progress.dropped.fetch_add(1, Ordering::Relaxed);
            self.progress.active.store(false, Ordering::Release);
        } else {
            self.progress.frames.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
pub(crate) struct Recorder {
    pub sink: Mutex<Option<RecordingSink>>,
    control: Mutex<Option<JoinHandle<()>>>,
    progress: Arc<Progress>,
    metadata: Mutex<Option<(PathBuf, u32)>>,
}

impl Recorder {
    pub fn status(&self) -> RecordingStatus {
        let meta = self.metadata.lock().expect("recording metadata");
        RecordingStatus {
            active: self.progress.active.load(Ordering::Acquire),
            path: meta.as_ref().map(|m| m.0.clone()),
            seconds: meta.as_ref().map_or(0., |m| {
                self.progress.frames.load(Ordering::Relaxed) as f64 / m.1 as f64
            }),
            dropped_frames: self.progress.dropped.load(Ordering::Relaxed),
            error: self.progress.error.lock().expect("recording error").clone(),
        }
    }

    pub fn start(&self, path: &Path, sample_rate: u32) -> Result<(), String> {
        let mut control = self
            .control
            .lock()
            .map_err(|_| "Recording control unavailable")?;
        if self.progress.active.load(Ordering::Acquire) {
            return Err("Already recording".into());
        }
        if let Some(worker) = control.take() {
            let _ = worker.join();
        }
        self.sink
            .lock()
            .map_err(|_| "Recording sink unavailable")?
            .take();
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let mut writer = hound::WavWriter::new(
            BufWriter::new(file),
            hound::WavSpec {
                channels: 2,
                sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .map_err(|e| e.to_string())?;
        let (producer, mut consumer) = rtrb::RingBuffer::new(sample_rate as usize * 2);
        let progress = self.progress.clone();
        progress.frames.store(0, Ordering::Relaxed);
        progress.dropped.store(0, Ordering::Relaxed);
        *progress.error.lock().unwrap() = None;
        progress.active.store(true, Ordering::Release);
        let worker = std::thread::Builder::new().name("mixless-recording".into()).spawn(move || {
            let result = (|| -> Result<(), String> {
                loop {
                    while let Ok(frame) = consumer.pop() {
                        for sample in frame { writer.write_sample(sample).map_err(|e| e.to_string())?; }
                    }
                    if !progress.active.load(Ordering::Acquire) || consumer.is_abandoned() { break; }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Ok(())
            })();
            // Always attempt to seal the header, even if the disk filled up.
            let finalized = writer.finalize().map_err(|e| e.to_string());
            let error = result.err().or_else(|| finalized.err()).or_else(|| {
                (progress.dropped.load(Ordering::Relaxed) > 0).then(||
                    "Recording stopped because capture could not keep up or the sample rate changed".into())
            });
            *progress.error.lock().unwrap() = error;
            progress.active.store(false, Ordering::Release);
        }).map_err(|e| {
            self.progress.active.store(false, Ordering::Release);
            e.to_string()
        })?;
        *self.metadata.lock().unwrap() = Some((path.to_path_buf(), sample_rate));
        *self.sink.lock().unwrap() = Some(RecordingSink {
            producer,
            progress: self.progress.clone(),
            sample_rate,
        });
        *control = Some(worker);
        Ok(())
    }

    pub fn stop(&self) -> Result<RecordingStatus, String> {
        let mut control = self
            .control
            .lock()
            .map_err(|_| "Recording control unavailable")?;
        // Wait for any in-flight block before ending the producer. The audio
        // callback never waits for us; only the host uses this blocking lock.
        self.sink
            .lock()
            .map_err(|_| "Recording sink unavailable")?
            .take();
        self.progress.active.store(false, Ordering::Release);
        if let Some(worker) = control.take() {
            worker
                .join()
                .map_err(|_| "Recording worker stopped unexpectedly")?;
        }
        let status = self.status();
        if let Some(error) = &status.error {
            return Err(error.clone());
        }
        Ok(status)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl super::Engine {
    /// Call on a host worker. Existing files are never overwritten.
    pub fn start_recording(&self, path: &Path) -> Result<(), String> {
        let _apply = self
            .audio_apply
            .lock()
            .map_err(|_| "Audio control unavailable")?;
        self.shared.recorder.start(path, self.sample_rate())
    }
    pub fn stop_recording(&self) -> Result<RecordingStatus, String> {
        self.shared.recorder.stop()
    }
    pub fn recording_status(&self) -> RecordingStatus {
        self.shared.recorder.status()
    }
}
