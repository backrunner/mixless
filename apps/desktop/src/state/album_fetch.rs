//! Fill missing album names from exact Spotify recording identities.
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

use mixless_library::Library;
use mixless_protocol::TrackId;
use mixless_spotify::{AlbumClient, SpotifyError};

use super::AppCore;

pub(super) struct AlbumFetch {
    requests: SyncSender<()>,
    results: Receiver<Result<usize, String>>,
}

impl AlbumFetch {
    pub(super) fn start(core: Arc<AppCore>) -> Self {
        let (requests, request_rx) = mpsc::sync_channel(1);
        let (result_tx, results) = mpsc::channel();
        std::thread::spawn(move || {
            let client = AlbumClient;
            let mut state = FetchState::default();
            loop {
                if core.shutting_down.load(Ordering::Acquire) {
                    break;
                }
                state.run(
                    &core.library,
                    Instant::now(),
                    |id| client.fetch(id),
                    |result| {
                        let _ = result_tx.send(result);
                    },
                    || core.shutting_down.load(Ordering::Acquire),
                );
                // New imports/retries wake this worker. A timer also recovers
                // temporary network failures without repeatedly hammering Spotify.
                if matches!(
                    request_rx.recv_timeout(Duration::from_secs(60)),
                    Err(mpsc::RecvTimeoutError::Disconnected)
                ) {
                    break;
                }
            }
        });
        Self { requests, results }
    }

    pub(super) fn refresh(&self) {
        let _ = self.requests.try_send(());
    }

    pub(super) fn poll(&self) -> usize {
        self.results
            .try_iter()
            .map(|result| match result {
                Ok(count) => count,
                Err(error) => {
                    tracing::warn!("album backfill: {error}");
                    0
                }
            })
            .sum()
    }
}

#[derive(Default)]
struct FetchState {
    retry_at: HashMap<(TrackId, String, String), Instant>,
    service_retry_at: Option<Instant>,
}

impl FetchState {
    fn run(
        &mut self,
        library: &Library,
        now: Instant,
        mut fetch: impl FnMut(&str) -> Result<Option<String>, SpotifyError>,
        mut report: impl FnMut(Result<usize, String>),
        cancelled: impl Fn() -> bool,
    ) {
        if self.service_retry_at.is_some_and(|retry| now < retry) {
            return;
        }
        let started = Instant::now();
        self.retry_at.retain(|_, retry| now < *retry);
        let targets = match library.missing_spotify_albums() {
            Ok(targets) => targets,
            Err(error) => {
                report(Err(error.to_string()));
                return;
            }
        };
        for target in targets {
            if cancelled() {
                break;
            }
            let key = (
                target.track_id,
                target.content_hash.clone(),
                target.spotify_id.clone(),
            );
            if self.retry_at.contains_key(&key) {
                continue;
            }
            let response = match target.album.as_deref().filter(|s| !s.trim().is_empty()) {
                Some(album) => Ok(Some(album.to_owned())),
                None => fetch(&target.spotify_id),
            };
            let finished = now + started.elapsed();
            let album = match response {
                Ok(Some(album)) => album,
                Ok(None) => {
                    self.retry_at
                        .insert(key, finished + Duration::from_secs(24 * 3600));
                    continue;
                }
                Err(error) => {
                    let wait = match error {
                        SpotifyError::RateLimited(seconds) => seconds.clamp(1, 86400),
                        _ => 60,
                    };
                    self.service_retry_at = Some(finished + Duration::from_secs(wait));
                    self.retry_at
                        .insert(key, finished + Duration::from_secs(wait.max(300)));
                    report(Err(error.to_string()));
                    break;
                }
            };
            let result = library
                .cache_spotify_album(&target, &album)
                .map_err(|e| e.to_string());
            match result {
                Ok(true) => report(Ok(1)),
                Ok(false) => {} // Removed, changed, or another worker supplied an album.
                Err(error) => {
                    self.retry_at
                        .insert(key, finished + Duration::from_secs(300));
                    report(Err(error));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limits_wait_and_success_stops_refetching() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(&dir.path().join("library.db")).unwrap();
        let audio = dir.path().join("song.mp3");
        std::fs::write(&audio, b"audio fixture").unwrap();
        let id = library.import_file(&audio).unwrap();
        let playlist = library.external_playlist("spotify", "set", "Set").unwrap();
        library
            .save_import_items(
                playlist,
                &[mixless_library::ImportItem {
                    position: 0,
                    external_id: "a".repeat(22),
                    title: "Song".into(),
                    artist: "Artist".into(),
                    album: None,
                    duration_ms: 1000,
                    status: "acquired".into(),
                    track_id: Some(id),
                    error: None,
                }],
            )
            .unwrap();
        let mut state = FetchState::default();
        let now = Instant::now();
        let mut reports = vec![];
        state.run(
            &library,
            now,
            |_| Err(SpotifyError::RateLimited(120)),
            |r| reports.push(r),
            || false,
        );
        assert!(reports[0].is_err());
        state.run(
            &library,
            now + Duration::from_secs(30),
            |_| panic!("service cooldown"),
            |_| {},
            || false,
        );
        state.run(
            &library,
            now + Duration::from_secs(150),
            |_| panic!("recording cooldown"),
            |_| {},
            || false,
        );
        state.run(
            &library,
            now + Duration::from_secs(301),
            |remote| {
                assert_eq!(remote, "a".repeat(22));
                Ok(Some("Recovered album".into()))
            },
            |r| reports.push(r),
            || false,
        );
        assert_eq!(*reports[1].as_ref().unwrap(), 1);
        assert_eq!(
            library.get_track(id).unwrap().album.as_deref(),
            Some("Recovered album")
        );
        state.run(
            &library,
            now + Duration::from_secs(600),
            |_| panic!("already filled"),
            |_| {},
            || false,
        );
    }
}
