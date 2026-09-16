//! Offline native inference. Never call from the UI or audio callback.
mod cache;
mod evidence;
mod lock;
mod models;
#[cfg(stems_ort)]
mod notes;
mod playback;
mod priority;
mod processor;
mod resample;
#[cfg(stems_ort)]
mod separation;
pub use evidence::VERSION;
pub use processor::Processor;

/// False on Intel macOS: no ONNX Runtime build exists for
/// x86_64-apple-darwin, so separation jobs fail there — cached stems and
/// cached analyses still load and play.
pub fn inference_supported() -> bool {
    cfg!(stems_ort)
}

pub fn is_current(analysis: &mixless_protocol::StemAnalysis) -> bool {
    analysis.version == VERSION
        && analysis.separator_sha256 == models::SEPARATOR_HASH
        && analysis.notes_sha256 == models::NOTES_HASH
        && analysis.valid(analysis.duration_sec)
}

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
    separator: ort::session::Session,
    notes: ort::session::Session,
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
        download: bool,
        progress: &mut impl FnMut(Progress),
        active: &impl Fn() -> bool,
    ) -> Result<Self> {
        let paths = models::ensure(model_dir, download, progress, active)?;
        progress(Progress::Loading);
        // Default keeps at least two cores free for the audio callback and UI;
        // the env override exists for throughput benchmarking.
        let detected = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let intra = std::env::var("MIXLESS_ORT_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(detected.saturating_sub(2).min(4).max(1))
            .clamp(1, 16);
        let session = |path| -> Result<_> {
            Ok(ort::session::Session::builder()?
                .with_intra_threads(intra)?
                .with_inter_threads(1)?
                .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
                .commit_from_file(path)?)
        };
        let separator = session(&paths.separator)?;
        check(active)?;
        let notes = session(&paths.notes)?;
        Ok(Self { separator, notes })
    }
}
