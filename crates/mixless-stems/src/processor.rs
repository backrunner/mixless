use crate::{cache, check, Error, Inference, Progress, Result};
use mixless_protocol::{StemAnalysis, StemKind, StemNote};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

/// One inference job per processor. Waiting workers do not decode or allocate models.
pub struct Processor {
    models: PathBuf,
    cache: PathBuf,
    download: bool,
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    inference: Option<Inference>,
    failed: Option<(Instant, String)>,
}

impl Processor {
    pub fn new(models: PathBuf, cache: PathBuf, download: bool) -> Self {
        Self {
            models,
            cache,
            download,
            state: Mutex::new(State::default()),
        }
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
        let mut state = loop {
            check(active)?;
            match self.state.try_lock() {
                Ok(s) => break s,
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(25))
                }
                Err(_) => return Err(Error::Model("Model worker lock poisoned".into())),
            }
        };
        std::fs::create_dir_all(&self.cache)?;
        let _process = crate::lock::acquire(&self.cache.join(".analysis.lock"), active)?;
        if let Some(a) = self.cached(content_hash, duration)? {
            return Ok(a);
        }
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
        let audio = load()?;
        if (audio.frames as f64 / audio.sample_rate.max(1) as f64 - duration as f64).abs() > 0.05 {
            return Err(Error::Model(
                "Audio duration changed before separation".into(),
            ));
        }
        let inference = state.inference.as_mut().unwrap();
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
        cache::prune(&self.cache, &path);
        Ok(analysis)
    }
    pub fn invalidate(&self, content_hash: &str) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Model("Model worker lock poisoned".into()))?;
        std::fs::create_dir_all(&self.cache)?;
        let _process = crate::lock::acquire(&self.cache.join(".analysis.lock"), &|| true)?;
        let path = self.cache_path(content_hash);
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
        state.failed = None;
        Ok(())
    }
    pub fn model_dir(&self) -> &Path {
        &self.models
    }
    pub fn retry(&self) {
        if let Ok(mut state) = self.state.try_lock() {
            state.failed = None;
        }
    }
    pub fn release_models(&self) {
        if let Ok(mut state) = self.state.try_lock() {
            state.inference = None;
        }
    }
}
