//! Restore Spotify covers independently of acquisition, analysis and playback.
use std::{
    collections::HashMap,
    io::Cursor,
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use mixless_library::Library;
use mixless_protocol::TrackId;
use mixless_spotify::{ArtworkClient, SpotifyError};

use super::AppCore;

pub(super) struct ArtworkFetch {
    requests: SyncSender<()>,
    results: Receiver<Result<usize, String>>,
}

impl ArtworkFetch {
    pub(super) fn start(core: Arc<AppCore>) -> Self {
        let (requests, request_rx) = mpsc::sync_channel(1);
        let (result_tx, results) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = result_tx.send(core.library.backfill_artwork().map_err(|e| e.to_string()));
            let client = ArtworkClient::default();
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
                    tracing::warn!("artwork backfill: {error}");
                    0
                }
            })
            .sum()
    }
}

#[derive(Default)]
struct FetchState {
    retry_at: HashMap<(TrackId, String), Instant>,
    service_retry_at: Option<Instant>,
}

impl FetchState {
    fn run(
        &mut self,
        library: &Library,
        now: Instant,
        mut fetch: impl FnMut(&str) -> Result<Option<Vec<u8>>, SpotifyError>,
        mut report: impl FnMut(Result<usize, String>),
        cancelled: impl Fn() -> bool,
    ) {
        if self.service_retry_at.is_some_and(|retry| now < retry) {
            return;
        }
        let started = Instant::now();
        self.retry_at.retain(|_, retry| now < *retry);
        let targets = match library.missing_spotify_artwork() {
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
            let key = (target.track_id, target.content_hash.clone());
            if self.retry_at.contains_key(&key) {
                continue;
            }
            let response = fetch(&target.spotify_id);
            let finished = now + started.elapsed();
            let bytes = match response {
                Ok(Some(bytes)) => bytes,
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
            let result = validated_cover(&bytes).and_then(|bytes| {
                library
                    .cache_spotify_artwork(&target, &bytes)
                    .map_err(|e| e.to_string())
            });
            match result {
                Ok(true) => report(Ok(1)),
                Ok(false) => {} // Removed, changed, or another worker supplied a cover.
                Err(error) => {
                    self.retry_at
                        .insert(key, finished + Duration::from_secs(300));
                    report(Err(error));
                }
            }
        }
    }
}

fn validated_cover(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    image.apply_orientation(orientation);
    // Keep the complete cover and bound disk/GPU costs; do not alter the audio
    // file or invalidate its hash, beat grid or stems when artwork arrives.
    let image = if image.width() > 1024 || image.height() > 1024 {
        image.thumbnail(1024, 1024)
    } else {
        image
    };
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(output.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_library::ImportItem;

    fn cover() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(48, 32)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn link(core: &AppCore, track: TrackId, id: &str) {
        let playlist = core
            .library
            .external_playlist("spotify", id, "Cover test")
            .unwrap();
        core.library
            .save_import_items(
                playlist,
                &[ImportItem {
                    position: 0,
                    external_id: id.into(),
                    title: "Song".into(),
                    artist: "Artist".into(),
                    duration_ms: 1000,
                    status: "acquired".into(),
                    track_id: Some(track),
                    error: None,
                }],
            )
            .unwrap();
    }

    #[test]
    fn covers_fill_existing_and_later_imports_without_touching_audio_or_refetching() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        link(&core, tracks[0], &"a".repeat(22));
        let before = core.library.get_track(tracks[0]).unwrap();
        let mut state = FetchState::default();
        let now = Instant::now();
        let mut calls = 0;
        let mut updated = 0;
        for _ in 0..2 {
            state.run(
                &core.library,
                now,
                |id| {
                    assert_eq!(id, "a".repeat(22));
                    calls += 1;
                    Ok(Some(cover()))
                },
                |r| updated += r.unwrap(),
                || false,
            );
        }
        assert_eq!((calls, updated), (1, 1));
        let after = core.library.get_track(tracks[0]).unwrap();
        assert_eq!(before.content_hash, after.content_hash);
        assert_eq!(before.bpm, after.bpm);
        assert_eq!(before.analyzed, after.analyzed);
        assert!(after.artwork_path.is_some());
        link(&core, tracks[1], &"b".repeat(22));
        state.run(
            &core.library,
            now,
            |_| {
                calls += 1;
                Ok(Some(cover()))
            },
            |r| updated += r.unwrap(),
            || false,
        );
        assert_eq!((calls, updated), (2, 2));
    }

    #[test]
    fn rate_limits_and_bad_images_do_not_poison_imports_or_retry_on_every_refresh() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        link(&core, tracks[0], &"a".repeat(22));
        let mut state = FetchState::default();
        let now = Instant::now();
        state.run(
            &core.library,
            now,
            |_| Err(SpotifyError::RateLimited(600)),
            |r| assert!(r.is_err()),
            || false,
        );
        for elapsed in [1, 60, 599] {
            state.run(
                &core.library,
                now + Duration::from_secs(elapsed),
                |_| panic!("rate-limited"),
                |_| {},
                || false,
            );
        }
        state.run(
            &core.library,
            now + Duration::from_secs(601),
            |_| Ok(Some(b"not an image".to_vec())),
            |r| assert!(r.is_err()),
            || false,
        );
        assert!(
            core.library
                .get_track(tracks[0])
                .unwrap()
                .artwork_path
                .is_none()
        );
        state.run(
            &core.library,
            now + Duration::from_secs(602),
            |_| panic!("image retry cooldown"),
            |_| {},
            || false,
        );
        state.run(
            &core.library,
            now + Duration::from_secs(902),
            |_| Ok(Some(cover())),
            |r| assert_eq!(r.unwrap(), 1),
            || false,
        );
    }

    #[test]
    fn artwork_validation_bounds_decoded_dimensions_and_keeps_aspect_ratio() {
        let small = image::load_from_memory(&validated_cover(&cover()).unwrap()).unwrap();
        assert_eq!((small.width(), small.height()), (48, 32));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(1600, 800)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        let decoded = image::load_from_memory(&validated_cover(bytes.get_ref()).unwrap()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (1024, 512));
        let mut oversized = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(4097, 1)
            .write_to(&mut oversized, ImageFormat::Png)
            .unwrap();
        assert!(validated_cover(oversized.get_ref()).is_err());
    }

    #[test]
    #[ignore = "live Spotify artwork service; run explicitly"]
    fn live_spotify_cover_is_downloaded_validated_and_cached() {
        let playlist = mixless_spotify::SpotifyClient::new()
            .fetch_playlist("0bjQ85Dl4WPRHyWzsJNBA1")
            .unwrap();
        let remote = playlist
            .tracks
            .iter()
            .find(|track| !track.id.is_empty())
            .unwrap();
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        link(&core, tracks[0], &remote.id);
        let worker = ArtworkFetch::start(core.clone());
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut updated = 0;
        while updated == 0 && Instant::now() < deadline {
            updated += worker.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        core.shutting_down.store(true, Ordering::Release);
        drop(worker);
        assert_eq!(updated, 1);
        let path = core
            .library
            .get_track(tracks[0])
            .unwrap()
            .artwork_path
            .unwrap();
        assert!(image::open(path).unwrap().width() > 0);
    }
}
