//! Shared types for Mixless. No file I/O, FFI, or Tokio.

mod analysis;
mod buildup;
mod command;
mod event;
mod fx;
mod ids;
mod instrument_phrase;
pub use instrument_phrase::{
    instrument_boundaries, instrument_voices, InstrumentBoundary, InstrumentBoundaryKind,
    InstrumentEvidence, InstrumentScope, InstrumentVoice,
};
pub mod inference_runtime;
mod master;
mod mixplan;
mod models;
pub use master::*;
mod phrases;
mod snapshot;
mod stems;
mod tonal;
mod vocal_pattern;
pub use vocal_pattern::{vocal_pattern, VocalPattern};

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
    relative_recoveries, released_peak, PeakEvidence, PeakScope,
};
pub use snapshot::*;
pub use stems::*;
pub use tonal::{
    estimate_harmonic_key, estimate_key, key_candidates, local_tonality, tonal_intervals,
    tonal_profile, tonal_windows, transpose_camelot, KeyCandidate, TonalProfile, TonalWindow,
};

mod workers;
pub use workers::{BackgroundCpuPermit, BackgroundWorkers};

mod energy;
pub use energy::{bar_energy, energy_reference, musical_energy, track_energy, MusicalEnergy};
