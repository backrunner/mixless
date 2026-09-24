//! Portable, incremental `.mixpack` packages. Streaming I/O belongs on a worker.
mod container;
mod export;
mod import;
use container::{Blob, Chunk, Container};
pub use export::export;
pub use import::import;
use mixless_library::{
    portable::{PortablePlaylist, PortableTrack},
    Library, LibraryError,
};
use mixless_protocol::{PlaylistId, TrackId};
use mixless_stems::Processor;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Library(#[from] LibraryError),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Stems(#[from] mixless_stems::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("Package operation cancelled")]
    Cancelled,
}
fn check(active: &impl Fn() -> bool) -> Result<()> {
    if active() {
        Ok(())
    } else {
        Err(Error::Cancelled)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    chunks: BTreeMap<String, Chunk>,
    sources: BTreeMap<String, Source>,
}
#[derive(Default, Serialize, Deserialize)]
struct Source {
    tracks: BTreeMap<i64, PackedTrack>,
    playlists: BTreeMap<i64, PortablePlaylist>,
    order: Vec<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
struct PackedTrack {
    record: Blob,
    audio: Blob,
    extension: String,
    artwork: Option<Blob>,
    stems: Option<Vec<Blob>>,
}

#[derive(Clone, Debug)]
pub enum Selection {
    Library,
    Playlists(Vec<PlaylistId>),
}
#[derive(Default, Debug)]
pub struct Report {
    pub tracks: usize,
    pub playlists: usize,
    pub stems: usize,
    pub without_analysis: usize,
    pub without_stems: usize,
    pub added_bytes: u64,
    pub generation: u64,
    pub imported: Vec<TrackId>,
}

fn validate_extension(extension: &str) -> Result<()> {
    if extension.is_empty()
        || extension.len() > 12
        || !extension.bytes().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(Error::Invalid("Invalid audio extension".into()));
    }
    Ok(())
}
#[cfg(test)]
mod tests;
