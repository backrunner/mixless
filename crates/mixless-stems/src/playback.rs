//! Loading/resampling only, off the UI and audio callback; never starts inference.
use crate::{cache, Error, Processor, Result};
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
        if sample_rate == 0 || frames.saturating_mul(16) > MAX_BYTES {
            return Err(Error::Model(
                "Track exceeds the 512 MiB stem playback limit per deck".into(),
            ));
        }
        let root = self.cache_path(hash);
        let Some(parent) = root.parent().filter(|p| p.is_dir()) else {
            return Ok(None);
        };
        // Immutable cache files are published by rename. Never wait behind the
        // process-wide inference lock: a cache miss must not delay AutoMix.
        let _ = parent;
        if cache::read(&root, frames as f32 / sample_rate as f32)?.is_none() {
            return Ok(None);
        }
        let read = |name: &str| -> Result<Vec<f32>> {
            let mut wav = hound::WavReader::open(root.join(format!("{name}.wav")))?;
            let spec = wav.spec();
            if spec.channels != 2
                || spec.sample_rate != 44100
                || spec.bits_per_sample != 32
                || spec.sample_format != hound::SampleFormat::Float
            {
                return Err(Error::Model("Unsupported stem cache format".into()));
            }
            let samples: Vec<f32> = wav
                .samples::<f32>()
                .collect::<std::result::Result<_, _>>()?;
            let mut aligned = crate::resample::convert(&samples, 2, 44100, sample_rate);
            if aligned.len().abs_diff(frames as usize * 2) > 4 {
                return Err(Error::Model("Stem duration alignment mismatch".into()));
            }
            aligned.resize(frames as usize * 2, 0.);
            Ok(aligned)
        };
        let audio = mixless_engine::StemBuffer::new(sample_rate, read("vocals")?, read("drums")?)
            .map_err(|e| Error::Model(e.into()))?;
        Ok(Some(Arc::new(audio)))
    }
}
