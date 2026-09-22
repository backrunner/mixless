//! Shared types for Mixless. No file I/O, FFI, or Tokio.

mod analysis;
mod buildup;
mod command;
mod event;
mod fx;
mod ids;
mod mixplan;
mod models;
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
pub use models::{ModelArtifact, STEM_MODELS, STEM_NOTES, STEM_SEPARATOR};
pub use phrases::{drop_suspension, peak_ranges};
pub use snapshot::*;
pub use stems::*;
pub use tonal::estimate_key;

mod workers;
pub use workers::{BackgroundCpuPermit, BackgroundWorkers};
