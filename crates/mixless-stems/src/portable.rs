//! Cache transfer is available on every platform and never loads a model.
use crate::{cache, Error, Processor, Result};
use std::{fs, path::Path};

pub const FILES: [&str; 5] = [
    "analysis.json",
    "vocals.wav",
    "drums.wav",
    "instruments.wav",
    "bass.wav",
];

impl Processor {
    pub fn with_portable_cache<T>(
        &self,
        hash: &str,
        duration: f32,
        active: &impl Fn() -> bool,
        read: impl FnOnce(&Path) -> T,
    ) -> Result<Option<T>> {
        fs::create_dir_all(&self.cache)?;
        let _shared = crate::lock::acquire_shared(&self.cache.join(".analysis.lock"), active)?;
        let _content = crate::lock::acquire(
            &self.cache.join(format!(".{}.lock", cache::key(hash))),
            active,
        )?;
        let path = self.cache_path(hash);
        Ok(cache::read(&path, duration)?.map(|_| read(&path)))
    }

    pub fn validate_portable_cache(
        path: &Path,
        duration: f32,
        active: &impl Fn() -> bool,
    ) -> Result<()> {
        crate::check(active)?;
        if cache::read(path, duration)?.is_none() {
            return Err(Error::Model(
                "Package contains incompatible or incomplete stems".into(),
            ));
        }
        // Playback must never receive non-finite samples from an imported file.
        for name in &FILES[1..] {
            let mut wav = hound::WavReader::open(path.join(name))?;
            if (wav.duration() as f64 - duration as f64 * 44100.).abs() > 4. {
                return Err(Error::Model(
                    "Package stem duration is not aligned with its track".into(),
                ));
            }
            for (index, sample) in wav.samples::<f32>().enumerate() {
                if index % 16384 == 0 {
                    crate::check(active)?;
                }
                if !sample?.is_finite() {
                    return Err(Error::Model(
                        "Package contains non-finite stem audio".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn install_portable_cache(
        &self,
        hash: &str,
        duration: f32,
        source: &Path,
        active: &impl Fn() -> bool,
    ) -> Result<()> {
        Self::validate_portable_cache(source, duration, active)?;
        fs::create_dir_all(&self.cache)?;
        let _shared = crate::lock::acquire_shared(&self.cache.join(".analysis.lock"), active)?;
        let _content = crate::lock::acquire(
            &self.cache.join(format!(".{}.lock", cache::key(hash))),
            active,
        )?;
        let target = self.cache_path(hash);
        if cache::read(&target, duration)?.is_some() {
            return Ok(());
        }
        let staging = tempfile::Builder::new()
            .prefix(".import-")
            .tempdir_in(&self.cache)?;
        for name in FILES {
            crate::check(active)?;
            fs::copy(source.join(name), staging.path().join(name))?;
            fs::File::open(staging.path().join(name))?.sync_all()?;
        }
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        fs::rename(staging.path(), target)?;
        #[cfg(unix)]
        fs::File::open(&self.cache)?.sync_all()?;
        Ok(())
    }
}
