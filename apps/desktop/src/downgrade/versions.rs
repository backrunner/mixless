use serde::{Deserialize, Serialize};

use super::assets::{CacheFile, ModelFile};

/// This records the last authorized writer, not a permanent high-water mark:
/// accepting a downgrade must not prompt again on every launch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Stamp {
    pub format: u32,
    pub app: String,
    pub built: String,
    pub schema: u32,
    pub analysis: u32,
    pub stems: u32,
    #[serde(default)]
    pub stem_storage: u32,
    pub order: u32,
    pub waveform: u32,
    #[serde(default)]
    pub cues: u32,
    pub runtime: String,
    pub models: Vec<ModelFile>,
    #[serde(default)]
    pub caches: Vec<CacheFile>,
}

impl Stamp {
    pub fn current() -> Self {
        Self {
            format: 1,
            app: env!("CARGO_PKG_VERSION").into(),
            built: env!("MIXLESS_BUILD_TIME").into(),
            schema: mixless_library::SCHEMA_VERSION,
            analysis: mixless_analyze::ANALYSIS_VERSION,
            stems: mixless_stems::VERSION,
            stem_storage: mixless_stems::STORAGE_VERSION,
            order: mixless_mixplan::order::ORDER_VERSION,
            waveform: mixless_library::WAVEFORM_VERSION,
            cues: mixless_library::AUTO_CUE_VERSION,
            runtime: mixless_protocol::inference_runtime::VERSION.into(),
            models: Vec::new(),
            caches: Vec::new(),
        }
    }

    pub fn newer_than(&self, current: &Self) -> bool {
        self.format > current.format
            || newer_app(&self.app, &current.app)
            || (self.app == current.app && self.built > current.built)
            || self.schema > current.schema
            || self.analysis > current.analysis
            || self.stems > current.stems
            || self.stem_storage > current.stem_storage
            || self.order > current.order
            || self.waveform > current.waveform
            || self.cues > current.cues
            || newer_runtime(&self.runtime, &current.runtime)
    }
}

fn newer_runtime(stored: &str, current: &str) -> bool {
    if stored == current {
        return false;
    }
    let parse = |s: &str| -> Option<(semver::Version, semver::Version)> {
        let (ort, mlx) = s.strip_prefix("ort-")?.split_once("-mlx-")?;
        Some((
            semver::Version::parse(ort).ok()?,
            semver::Version::parse(mlx).ok()?,
        ))
    };
    match (parse(stored), parse(current)) {
        (Some((a, b)), Some((c, d))) => a > c || b > d,
        _ => true,
    }
}

pub(super) fn newer_app(stored: &str, current: &str) -> bool {
    match (
        semver::Version::parse(stored),
        semver::Version::parse(current),
    ) {
        (Ok(stored), Ok(current)) => stored > current,
        _ => stored != current,
    }
}

pub(super) fn library_versions() -> mixless_library::downgrade::Versions<'static> {
    mixless_library::downgrade::Versions {
        analysis: mixless_analyze::ANALYSIS_VERSION,
        stems: mixless_stems::VERSION,
        separator: mixless_protocol::STEM_SEPARATOR_HASH,
        notes: mixless_protocol::STEM_NOTES.sha256,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_app_build_and_each_independent_generation_detect_downgrades() {
        let current = Stamp::current();
        assert!(!current.newer_than(&current));
        for mutate in [
            |s: &mut Stamp| s.built = "2099-01-01T00:00:00Z".into(),
            |s: &mut Stamp| s.schema += 1,
            |s: &mut Stamp| s.analysis += 1,
            |s: &mut Stamp| s.stems += 1,
            |s: &mut Stamp| s.stem_storage += 1,
            |s: &mut Stamp| s.order += 1,
            |s: &mut Stamp| s.waveform += 1,
            |s: &mut Stamp| s.cues += 1,
            |s: &mut Stamp| s.runtime = "ort-99.0.0-mlx-0.29.6".into(),
            |s: &mut Stamp| s.format += 1,
        ] {
            let mut future = current.clone();
            mutate(&mut future);
            assert!(future.newer_than(&current));
        }
        assert!(newer_app("1.0.0", "1.0.0-beta.1"));
        assert!(!newer_app("1.0.0-beta.1", "1.0.0"));
        assert!(!newer_runtime("ort-1.28.0-mlx-0.28.0", &current.runtime));
    }
}
