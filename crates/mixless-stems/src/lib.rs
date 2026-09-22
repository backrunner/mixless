//! Offline native inference. Never call from the UI or audio callback.
mod cache;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod coreml;
mod evidence;
mod lock;
mod models;
#[cfg(stems_ort)]
mod notes;
mod playback;
mod priority;
mod processor;
mod resample;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod runtime;
#[cfg(stems_ort)]
mod separation;
#[cfg(stems_ort)]
mod session;
#[cfg(stems_ort)]
mod worker;
pub use evidence::VERSION;
pub use processor::Processor;

/// False on Intel macOS: no ONNX Runtime build exists for
/// x86_64-apple-darwin, so separation jobs fail there — cached stems and
/// cached analyses still load and play.
pub fn inference_supported() -> bool {
    cfg!(stems_ort)
}

/// Load and execute both bundled sessions without network or model cache writes.
/// Used by the packaged-app release check, including from a read-only DMG.
pub fn verify_bundled_models(dir: &std::path::Path) -> Result<()> {
    #[cfg(stems_ort)]
    {
        let mut inference =
            Inference::load_with_cache(dir, Some(dir), false, None, &mut |_| {}, &|| true)?;
        inference.separator.run(
            "mix",
            ort::value::Tensor::from_array(([1usize, 2, 343980], vec![0f32; 2 * 343980]))?,
            &["stems"],
            &|| true,
        )?;
        inference.notes.run(
            "serving_default_input_2:0",
            ort::value::Tensor::from_array(([1usize, 43844, 1], vec![0f32; 43844]))?,
            &["StatefulPartitionedCall:1", "StatefulPartitionedCall:2"],
            &|| true,
        )?;
        eprintln!(
            "Verified native model execution: {:?}",
            inference.backends()
        );
        Ok(())
    }
    #[cfg(not(stems_ort))]
    {
        let _ = dir;
        Err(Error::Model(
            "Stem separation requires Apple Silicon".into(),
        ))
    }
}

pub fn is_current(analysis: &mixless_protocol::StemAnalysis) -> bool {
    analysis.version == VERSION
        && analysis.separator_sha256 == models::SEPARATOR_HASH
        && analysis.notes_sha256 == models::NOTES_HASH
        && analysis.valid(analysis.duration_sec)
}

#[cfg(stems_ort)]
use std::path::Path;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Network(#[from] reqwest::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Wav(#[from] hound::Error),
    #[error("{0}")]
    Model(String),
    #[error("Analysis cancelled")]
    Cancelled,
}
#[cfg(stems_ort)]
impl<T> From<ort::Error<T>> for Error {
    fn from(error: ort::Error<T>) -> Self {
        Self::Model(format!("Inference: {error}"))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Downloading { model: String, percent: u8 },
    Loading,
    Separating(u8),
    Notes { stem: &'static str, percent: u8 },
    Saving,
}

pub(crate) fn check(active: &impl Fn() -> bool) -> Result<()> {
    if active() {
        Ok(())
    } else {
        Err(Error::Cancelled)
    }
}

#[cfg(stems_ort)]
pub struct Inference {
    separator: worker::ModelSession,
    notes: worker::ModelSession,
    threads: usize,
}

pub struct Stems {
    /// Interleaved stereo at 44.1 kHz, in vocals / drums / instruments order.
    pub audio: [Vec<f32>; 3],
    pub residual_rms: f32,
}

#[cfg(stems_ort)]
impl Inference {
    pub fn load(
        model_dir: &Path,
        bundled: Option<&Path>,
        download: bool,
        progress: &mut impl FnMut(Progress),
        active: &impl Fn() -> bool,
    ) -> Result<Self> {
        Self::load_with_cache(
            model_dir,
            bundled,
            download,
            Some(&model_dir.join(".coreml")),
            progress,
            active,
        )
    }

    pub(crate) fn load_with_cache(
        model_dir: &Path,
        bundled: Option<&Path>,
        download: bool,
        coreml_cache: Option<&Path>,
        progress: &mut impl FnMut(Progress),
        active: &impl Fn() -> bool,
    ) -> Result<Self> {
        let backend = session::Backend::environment()?;
        let paths = models::ensure(model_dir, bundled, download, progress, active)?;
        progress(Progress::Loading);
        // Each pooled session gets a share of the same CPU budget as basic
        // analysis; the override remains available for throughput benchmarks.
        let intra = std::env::var("MIXLESS_ORT_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(mixless_protocol::BackgroundWorkers::detected().inference_threads)
            .clamp(1, 16);
        tracing::info!(threads = intra, "Loading stem inference sessions");
        let separator = worker::ModelSession::load(
            &paths.separator,
            intra,
            "separator",
            backend,
            coreml_cache,
            active,
        )?;
        check(active)?;
        let notes = worker::ModelSession::load(
            &paths.notes,
            intra,
            "notes",
            backend,
            coreml_cache,
            active,
        )?;
        Ok(Self {
            separator,
            notes,
            threads: intra,
        })
    }

    /// Current session backends; accelerated providers also delegate unsupported nodes to CPU.
    pub fn backends(&self) -> (&'static str, &'static str) {
        (self.separator.backend(), self.notes.backend())
    }

    /// Flush optional ORT profiles for provider assignment and timing diagnostics.
    pub fn finish_profiling(&mut self) -> Result<Vec<String>> {
        Ok([
            self.separator.finish_profiling()?,
            self.notes.finish_profiling()?,
        ]
        .into_iter()
        .flatten()
        .collect())
    }
}
