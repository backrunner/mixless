mod destination;
pub mod imports;
pub mod local_paths;
pub use destination::playlist_download_dir;
mod matching;
pub use matching::{
    candidate_score, could_match, job_key, local_match, normalize, primary_artist, track_key,
    MatchKey,
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
