pub mod imports;
pub mod local_paths;
mod matching;
pub use matching::{
    MatchKey, candidate_score, could_match, job_key, local_match, normalize, track_key,
};
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

pub fn duration_ok(got_ms: u32, want_ms: u32) -> bool {
    got_ms > 0 && want_ms > 0 && got_ms.abs_diff(want_ms) <= 5000
}
