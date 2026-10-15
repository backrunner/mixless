//! Loading/resampling only, off the UI and audio callback; never starts inference.
use crate::{Error, Processor, Result, cache};
use std::sync::Arc;

impl Processor {
    pub fn playback(
        &self,
        hash: &str,
        frames: u64,
        sample_rate: u32,
    ) -> Result<Option<Arc<mixless_engine::StemBuffer>>> {
        // Keep memory bounded to two loaded decks, not a whole playlist of PCM.
        const MAX_BYTES: u64 = 512 * 1024 * 1024;
        if sample_rate == 0 || frames.saturating_mul(24) > MAX_BYTES {
            return Err(Error::Model(
                "Track exceeds the 512 MiB stem playback limit per deck".into(),
            ));
        }
        let duration = frames as f32 / sample_rate as f32;
        let mut root = self.cache_path(hash);
        let mut legacy = false;
        if !crate::inference_supported() && cache::read(&root, duration)?.is_none() {
            root = self.cache.join(cache::key_version(hash, 1));
            legacy = true;
        }
        let Some(parent) = root.parent().filter(|p| p.is_dir()) else {
            return Ok(None);
        };
        // Immutable cache files are published by rename. Never wait behind the
        // process-wide inference lock: a cache miss must not delay AutoMix.
        let _ = parent;
        if (if legacy {
            cache::read_legacy(&root, duration)?
        } else {
            cache::read(&root, duration)?
        })
        .is_none()
        {
            return Ok(None);
        }
        let manifest = cache::manifest(&root)?;
        let read = |index: usize| -> Result<Vec<f32>> {
            let samples = cache::audio(&root, &manifest, index, &|| true)?;
            let mut aligned = crate::resample::convert(&samples, 2, 44100, sample_rate);
            if aligned.len().abs_diff(frames as usize * 2) > 4 {
                return Err(Error::Model("Stem duration alignment mismatch".into()));
            }
            aligned.resize(frames as usize * 2, 0.);
            Ok(aligned)
        };
        let audio = if legacy {
            mixless_engine::StemBuffer::new(sample_rate, read(0)?, read(1)?)
        } else {
            mixless_engine::StemBuffer::with_bass(sample_rate, read(0)?, read(1)?, read(3)?)
        }
        .map_err(|e| Error::Model(e.into()))?;
        Ok(Some(Arc::new(audio)))
    }
}
