use super::*;
use crate::{could_match, job_key, local_match, track_key, MatchKey};
use mixless_protocol::Track;

pub(super) struct LocalRecordings {
    tracks: Vec<Track>,
    keys: Vec<MatchKey>,
}

impl LocalRecordings {
    pub(super) fn read(library: &Library) -> Result<Self, String> {
        let tracks = library.list_tracks().map_err(|e| e.to_string())?;
        let keys = tracks.iter().map(track_key).collect();
        Ok(Self { tracks, keys })
    }
}

impl ImportService<'_> {
    /// Shared validation, local matching and acquisition for imports and retries.
    pub(super) fn recording(
        &self,
        job: &ResolveJob,
        previous: Option<TrackId>,
        local: &LocalRecordings,
        fetch: &impl Fn(&ResolveJob) -> Result<PathBuf, AcquireError>,
        mut stage: impl FnMut(&'static str) -> Result<(), String>,
    ) -> Result<(TrackId, bool), AudioFailure> {
        stage("resolving")?;
        if job.spotify_id.len() != 22
            || !job.spotify_id.bytes().all(|c| c.is_ascii_alphanumeric())
            || job.duration_ms == 0
        {
            return Err("unavailable track or missing recording metadata"
                .to_string()
                .into());
        }
        let linked = previous.or(self
            .library
            .linked_import_track(&job.spotify_id)
            .map_err(|e| e.to_string())?);
        let matched = linked
            .and_then(|id| {
                local
                    .tracks
                    .iter()
                    .find(|t| t.id == id && Path::new(&t.path).is_file())
            })
            .or_else(|| {
                let want = job_key(job);
                local
                    .tracks
                    .iter()
                    .zip(&local.keys)
                    .find(|(t, key)| {
                        could_match(&want, key)
                            && local_match(job, t)
                            && Path::new(&t.path).is_file()
                    })
                    .map(|(t, _)| t)
            });
        if let Some(track) = matched {
            stage("analyzing")?;
            if let Ok(id) = self.audio(Path::new(&track.path), Some(job.duration_ms)) {
                return Ok((id, true));
            }
            stage("resolving")?;
        }
        let path = fetch(job).map_err(|e| e.to_string())?;
        stage("analyzing")?;
        let id = match self.audio(&path, Some(job.duration_ms)) {
            Ok(id) => id,
            Err(error) => {
                if path
                    .file_stem()
                    .is_some_and(|s| s == job.spotify_id.as_str())
                {
                    let _ = std::fs::remove_file(&path);
                    let _ = std::fs::remove_file(path.with_extension("json"));
                }
                return Err(error);
            }
        };
        self.library
            .set_recording_metadata(id, &job.title, &job.artist, job.isrc.as_deref())
            .map_err(|e| e.to_string())?;
        Ok((id, false))
    }
}
