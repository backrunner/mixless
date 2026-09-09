//! Shared FX identities, metadata and persisted state. Published IDs are stable.

mod capabilities;
mod kind;
mod metadata;
mod state;

pub use kind::FxKind;
pub use state::{default_fx, FxState, FX_BEATS, FX_BEAT_LABELS};

#[cfg(test)]
mod tests;
