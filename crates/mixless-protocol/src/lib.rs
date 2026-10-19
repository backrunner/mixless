//! Shared types for Mixless. No file I/O, FFI, or Tokio.

mod analysis;
mod buildup;
mod command;
mod event;
mod fx;
mod ids;
pub mod inference_runtime;
mod master;
mod mixplan;
mod models;
pub use master::*;
mod phrases;
mod snapshot;
mod stems;
mod tonal;

pub use analysis::*;
pub use buildup::has_buildup;
pub use command::*;
pub use event::*;
pub use fx::*;
pub use ids::*;
pub use mixplan::*;
pub use models::{
    ModelArtifact, STEM_MODELS, STEM_NOTES, STEM_SEPARATOR, STEM_SEPARATORS, STEM_SEPARATOR_HASH,
};
pub use phrases::{
    drop_suspension, main_peak, major_peaks, peak_ranges, peak_units, percussion_release,
    released_peak,
};
pub use snapshot::*;
pub use stems::*;
pub use tonal::{estimate_harmonic_key, estimate_key, transpose_camelot};

mod workers;
pub use workers::{BackgroundCpuPermit, BackgroundWorkers};
