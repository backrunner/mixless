pub mod imports;
pub mod local_paths;
mod matching;
pub use matching::{candidate_score, local_match, normalize};
use mixless_protocol::BatchId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AcquireError {
    #[error("{0}")]
    Msg(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveJob {
    pub spotify_id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u32,
    pub isrc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcquireProgress {
    pub index: usize,
    pub total: usize,
    pub title: String,
    pub status: String,
    pub path: Option<String>,
}

pub fn duration_ok(got_ms: u32, want_ms: u32) -> bool {
    got_ms > 0 && want_ms > 0 && got_ms.abs_diff(want_ms) <= 5000
}

pub fn search_query(job: &ResolveJob) -> String {
    format!("{} {}", job.artist, job.title)
}

pub struct AcquireService;

impl AcquireService {
    pub fn new() -> Self {
        Self
    }

    pub fn enqueue(&self, _jobs: Vec<ResolveJob>) -> BatchId {
        BatchId(0)
    }
}

impl Default for AcquireService {
    fn default() -> Self {
        Self::new()
    }
}
