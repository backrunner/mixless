#[cfg(stems_ort)]
use crate::Inference;
use crate::{cache, check, Error, Progress, Result};
use mixless_protocol::StemAnalysis;
#[cfg(stems_ort)]
use mixless_protocol::{StemKind, StemNote};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Clearing waits behind an in-flight analysis, but not for a whole queue.
const LOCK_WAIT: Duration = Duration::from_secs(15);

fn busy(error: Error) -> Error {
    match error {
        Error::Cancelled => {
            Error::Model("Stem analysis is running; retry after it finishes".into())
        }
        error => error,
    }
}

/// A bounded pool of independent model sessions; waiting jobs do not decode PCM.
pub struct Processor {
    models: PathBuf,
    cache: PathBuf,
    #[cfg_attr(not(stems_ort), allow(dead_code))]
    download: bool,
    states: Vec<Mutex<State>>,
}
#[derive(Default)]
struct State {
    #[cfg(stems_ort)]
    inference: Option<Inference>,
    failed: Option<(Instant, String)>,
}

impl Processor {
    pub fn new(models: PathBuf, cache: PathBuf, download: bool) -> Self {
        Self {
            models,
            cache,
            download,
            states: (0..mixless_protocol::BackgroundWorkers::detected().stems)
                .map(|_| Mutex::new(State::default()))
                .collect(),
        }
    }
    pub fn parallelism(&self) -> usize {
        self.states.len()
    }

    pub fn cache_path(&self, content_hash: &str) -> PathBuf {
        self.cache.join(cache::key(content_hash))
    }
    pub fn cached(&self, content_hash: &str, duration: f32) -> Result<Option<StemAnalysis>> {
        cache::read(&self.cache_path(content_hash), duration)
    }
    pub fn analyze(
        &self,
        content_hash: &str,
        duration: f32,
        load: impl FnOnce() -> Result<Arc<mixless_engine::AudioBuffer>>,
        progress: &mut impl FnMut(Progress),
        active: &impl Fn() -> bool,
    ) -> Result<StemAnalysis> {
        crate::priority::background();
        check(active)?;
        if let Some(a) = self.cached(content_hash, duration)? {
            return Ok(a);
        }
        self.run(content_hash, duration, load, progress, active)
    }

    #[cfg(stems_ort)]
    fn run(
        &self,
        content_hash: &str,
        duration: f32,
        load: impl FnOnce() -> Result<Arc<mixless_engine::AudioBuffer>>,
        progress: &mut impl FnMut(Progress),
        active: &impl Fn() -> bool,
    ) -> Result<StemAnalysis> {
        std::fs::create_dir_all(&self.cache)?;
        // Different content can run concurrently. Same-content jobs coalesce
        // across workers/processes, and cache cleanup waits for all publishers.
        let _process = crate::lock::acquire_shared(&self.cache.join(".analysis.lock"), active)?;
        let _content = crate::lock::acquire(
            &self
                .cache
                .join(format!(".{}.lock", cache::key(content_hash))),
            active,
        )?;
        if let Some(a) = self.cached(content_hash, duration)? {
            return Ok(a);
        }
        let mut state = loop {
            check(active)?;
            let mut available = None;
            for slot in &self.states {
                match slot.try_lock() {
                    Ok(state) => {
                        available = Some(state);
                        break;
                    }
                    Err(std::sync::TryLockError::WouldBlock) => {}
                    Err(_) => return Err(Error::Model("Model worker lock poisoned".into())),
                }
            }
            if let Some(state) = available {
                break state;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        if let Some((at, error)) = &state.failed {
            if at.elapsed().as_secs() < 60 {
                return Err(Error::Model(error.clone()));
            }
        }
        if !duration.is_finite() || duration > 20. * 60. || duration <= 0. {
            return Err(Error::Model(
                "Deep analysis supports tracks up to 20 minutes".into(),
            ));
        }
        cache::reserve(&self.cache, (duration as f64 * 44100. * 24.).ceil() as u64)?;
        if state.inference.is_none() {
            match Inference::load(&self.models, self.download, progress, active) {
                Ok(i) => {
                    state.inference = Some(i);
                    state.failed = None;
                }
                Err(e) => {
                    if !matches!(e, Error::Cancelled) {
                        state.failed = Some((Instant::now(), e.to_string()));
                    }
                    return Err(e);
                }
            }
        }
        check(active)?;
        let inference = state.inference.as_mut().unwrap();
        let _cpu = mixless_protocol::BackgroundWorkers::acquire_cpu(inference.threads, active)
            .ok_or(Error::Cancelled)?;
        let audio = load()?;
        if (audio.frames as f64 / audio.sample_rate.max(1) as f64 - duration as f64).abs() > 0.05 {
            return Err(Error::Model(
                "Audio duration changed before separation".into(),
            ));
        }
        let stems = inference.separate(&audio.samples, audio.sample_rate, progress, active)?;
        drop(audio);
        let mut notes = Vec::new();
        for (index, name, stem) in [
            (0, "vocals", StemKind::Vocals),
            (2, "instruments", StemKind::Instruments),
        ] {
            notes.extend(
                inference
                    .transcribe(&stems.audio[index], name, progress, active)?
                    .into_iter()
                    .map(|n| StemNote {
                        stem,
                        start_sec: n.start_sec,
                        end_sec: n.end_sec,
                        midi: n.midi,
                        confidence: n.confidence,
                    }),
            );
        }
        let analysis = crate::evidence::extract(&stems, notes);
        if !analysis.valid(duration) {
            return Err(Error::Model("Incomplete or invalid stem analysis".into()));
        }
        check(active)?;
        progress(Progress::Saving);
        let path = self.cache_path(content_hash);
        cache::save(&path, &stems, &analysis)?;
        Ok(analysis)
    }

    #[cfg(not(stems_ort))]
    fn run(
        &self,
        content_hash: &str,
        duration: f32,
        load: impl FnOnce() -> Result<Arc<mixless_engine::AudioBuffer>>,
        progress: &mut impl FnMut(Progress),
        active: &impl Fn() -> bool,
    ) -> Result<StemAnalysis> {
        let _ = (content_hash, duration, load, progress, active);
        Err(Error::Model(
            "Stem separation requires Apple Silicon".into(),
        ))
    }

    /// Byte total of cached stems — every entry, or only the given content
    /// hashes. Hashes may repeat across playlists, so they are deduplicated.
    pub fn cache_usage(&self, content_hashes: Option<&[String]>) -> u64 {
        match content_hashes {
            Some(hashes) => hashes
                .iter()
                .collect::<HashSet<_>>()
                .into_iter()
                .map(|hash| cache::dir_bytes(&self.cache_path(hash)))
                .sum(),
            None => cache::dir_bytes(&self.cache),
        }
    }

    /// Delete cached stems — every entry, or only the given content hashes.
    /// A running analysis keeps ownership of the process lock; this waits
    /// briefly rather than interrupting it.
    pub fn clear_cache(&self, content_hashes: Option<&[String]>) -> Result<u64> {
        std::fs::create_dir_all(&self.cache)?;
        let deadline = Instant::now() + LOCK_WAIT;
        let _process = crate::lock::acquire(&self.cache.join(".analysis.lock"), &|| {
            Instant::now() < deadline
        })
        .map_err(busy)?;
        match content_hashes {
            Some(hashes) => {
                let mut freed = 0;
                for hash in hashes.iter().collect::<HashSet<_>>() {
                    let path = self.cache_path(hash);
                    freed += cache::dir_bytes(&path);
                    if path.exists() {
                        std::fs::remove_dir_all(path)?;
                    }
                }
                Ok(freed)
            }
            None => cache::clear_all(&self.cache),
        }
    }

    /// Delete downloaded inference models; they download again on next use.
    /// An already-loaded session is unaffected until it is released.
    pub fn clear_models(&self) -> Result<u64> {
        std::fs::create_dir_all(&self.models)?;
        let deadline = Instant::now() + LOCK_WAIT;
        let _lock = crate::lock::acquire(&self.models.join(".models.lock"), &|| {
            Instant::now() < deadline
        })
        .map_err(busy)?;
        let mut freed = 0;
        for entry in std::fs::read_dir(&self.models)?.flatten() {
            let path = entry.path();
            if entry.file_name().to_str() == Some(".models.lock") || !path.is_file() {
                continue;
            }
            freed += entry.metadata().map(|m| m.len()).unwrap_or(0);
            std::fs::remove_file(&path)?;
        }
        self.release_models();
        Ok(freed)
    }

    pub fn invalidate(&self, content_hash: &str) -> Result<()> {
        std::fs::create_dir_all(&self.cache)?;
        let _process = crate::lock::acquire(&self.cache.join(".analysis.lock"), &|| true)?;
        let path = self.cache_path(content_hash);
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
        self.retry();
        Ok(())
    }
    pub fn model_dir(&self) -> &Path {
        &self.models
    }
    pub fn retry(&self) {
        for slot in &self.states {
            if let Ok(mut state) = slot.try_lock() {
                state.failed = None;
            }
        }
    }
    pub fn release_models(&self) {
        #[cfg(stems_ort)]
        for slot in &self.states {
            if let Ok(mut state) = slot.try_lock() {
                state.inference = None;
            }
        }
    }
}
