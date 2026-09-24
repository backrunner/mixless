//! Import orchestration runs on a host worker. A row is ready only after the
//! engine's decoder and offline analysis have accepted the actual local file.
mod recording;
mod retry;
use crate::{duration_ok, AcquireError, ResolveJob};
use mixless_analyze::{Analyzer, ANALYSIS_VERSION};
use mixless_library::{content_hash, ImportItem, Library};
use mixless_protocol::{PlaylistId, TrackId};
use mixless_spotify::SpotifyPlaylistMeta;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

#[derive(Debug)]
pub enum ImportProgress {
    PlaylistReady(PlaylistId),
    Changed(PlaylistId, String),
}

#[derive(Debug, Default)]
pub struct ImportReport {
    pub total: usize,
    pub local: usize,
    pub acquired: usize,
    pub failed: usize,
    pub suspect: usize,
    pub errors: Vec<String>,
    pub warning: Option<String>,
    pub playlist: Option<PlaylistId>,
    pub sync: Option<(usize, usize)>,
}
impl ImportReport {
    pub fn summary(&self) -> String {
        let mut parts = vec![match self.sync {
            Some((added, removed)) => {
                format!("Updated playlist · {added} added · {removed} removed")
            }
            None => format!("Added {} tracks", self.local + self.acquired),
        }];
        let failed = self.failed + self.suspect;
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        parts.join(" · ")
    }
}

pub struct ImportService<'a> {
    pub library: &'a Library,
    pub analyzer: &'a Analyzer,
}
struct AudioFailure {
    message: String,
    suspect: bool,
}
impl From<String> for AudioFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            suspect: false,
        }
    }
}
impl ImportService<'_> {
    fn audio(&self, path: &Path, expected: Option<u32>) -> Result<TrackId, AudioFailure> {
        let canon = path.canonicalize().map_err(|e| e.to_string())?;
        let hash = content_hash(&canon).map_err(|e| e.to_string())?;
        let existing = self
            .library
            .track_at_path(&canon)
            .map_err(|e| e.to_string())?
            .filter(|t| t.content_hash == hash);
        let mut analysis = if let Some(t) = &existing {
            self.library
                .load_analysis(t.id, ANALYSIS_VERSION)
                .map_err(|e| e.to_string())?
        } else {
            None
        };
        if analysis.is_none() {
            analysis = Some(
                self.analyzer
                    .analyze_track(TrackId(0), &canon)
                    .map_err(|e| e.to_string())?,
            );
        }
        let mut analysis = analysis.unwrap();
        let duration_ms = (analysis.duration_sec * 1000.).round() as u32;
        if let Some(want) = expected {
            if !duration_ok(duration_ms, want) {
                return Err(AudioFailure {
                    message: format!(
                        "decoded duration {:.2}s differs from Spotify {:.2}s",
                        duration_ms as f64 / 1000.,
                        want as f64 / 1000.
                    ),
                    suspect: true,
                });
            }
        }
        if content_hash(&canon).map_err(|e| e.to_string())? != hash {
            return Err("file changed during analysis; retry import"
                .to_string()
                .into());
        }
        let id = if let Some(t) = existing {
            t.id
        } else {
            self.library
                .import_file(&canon)
                .map_err(|e| e.to_string())?
        };
        analysis.track_id = id;
        self.library
            .finish_analysis(&analysis, &hash, ANALYSIS_VERSION)
            .map_err(|e| e.to_string())?;
        Ok(id)
    }
    pub fn import_local_file(&self, path: &Path) -> Result<TrackId, String> {
        self.local_audio(path).map(|(track, _)| track)
    }
    fn local_audio(&self, path: &Path) -> Result<(TrackId, Option<PlaylistId>), String> {
        let track = self.audio(path, None).map_err(|e| e.message)?;
        let playlist = self
            .library
            .add_to_folder_playlist(track)
            .map_err(|e| e.to_string())?;
        Ok((track, playlist))
    }
    pub fn local_files(&self, paths: &[PathBuf], mut progress: impl FnMut(String)) -> ImportReport {
        let mut report = ImportReport {
            total: paths.len(),
            ..Default::default()
        };
        for (i, path) in paths.iter().enumerate() {
            progress(format!(
                "{} / {} · analyzing {}",
                i + 1,
                paths.len(),
                path.display()
            ));
            match self.local_audio(path) {
                Ok((_, playlist)) => {
                    report.local += 1;
                    if let Some(playlist) = playlist {
                        report.playlist.get_or_insert(playlist);
                    }
                }
                Err(error) => {
                    report.failed += 1;
                    report.errors.push(format!("{}: {error}", path.display()));
                }
            }
        }
        report
    }
    pub fn spotify_playlist(
        &self,
        playlist: &SpotifyPlaylistMeta,
        fetch: impl Fn(&ResolveJob) -> Result<PathBuf, AcquireError> + Sync,
        progress: impl FnMut(ImportProgress),
    ) -> Result<ImportReport, String> {
        self.spotify_snapshot(playlist, None, fetch, progress)
    }

    pub fn sync_spotify_playlist(
        &self,
        pid: PlaylistId,
        playlist: &SpotifyPlaylistMeta,
        fetch: impl Fn(&ResolveJob) -> Result<PathBuf, AcquireError> + Sync,
        progress: impl FnMut(ImportProgress),
    ) -> Result<ImportReport, String> {
        if playlist.total_tracks != Some(playlist.tracks.len()) || playlist.warning.is_some() {
            return Err("Spotify returned an incomplete playlist; nothing was changed".into());
        }
        self.spotify_snapshot(playlist, Some(pid), fetch, progress)
    }

    fn spotify_snapshot(
        &self,
        playlist: &SpotifyPlaylistMeta,
        sync: Option<PlaylistId>,
        fetch: impl Fn(&ResolveJob) -> Result<PathBuf, AcquireError> + Sync,
        mut progress: impl FnMut(ImportProgress),
    ) -> Result<ImportReport, String> {
        let pid = if let Some(pid) = sync {
            pid
        } else {
            self.library
                .external_playlist("spotify", &playlist.id, &playlist.name)
                .map_err(|e| e.to_string())?
        };
        let previous = self.library.import_items(pid).map_err(|e| e.to_string())?;
        let local = recording::LocalRecordings::read(self.library)?;
        let mut rows: Vec<_> = playlist
            .tracks
            .iter()
            .enumerate()
            .map(|(position, t)| ImportItem {
                position,
                external_id: t.id.clone(),
                title: t.title.clone(),
                artist: t.artist.clone(),
                duration_ms: t.duration_ms,
                status: "queued".into(),
                track_id: None,
                error: None,
            })
            .collect();
        let delta = sync.map(|_| snapshot_delta(&previous, &rows));
        if sync.is_some() {
            // Stable remote recording IDs retain healthy local tracks even
            // when their position changes or the playlist repeats a song.
            for row in &mut rows {
                if let Some(old) = previous.iter().find(|old| {
                    !row.external_id.is_empty()
                        && old.external_id == row.external_id
                        && old.is_ready()
                        && old
                            .track_id
                            .and_then(|id| self.library.get_track(id).ok())
                            .is_some_and(|t| {
                                t.analyzed
                                    && duration_ok(t.duration_ms, row.duration_ms)
                                    && self
                                        .library
                                        .verified_content_hash(Path::new(&t.path))
                                        .is_ok_and(|hash| hash == t.content_hash)
                            })
                }) {
                    row.status = old.status.clone();
                    row.track_id = old.track_id;
                }
            }
        }
        let mut report = ImportReport {
            total: rows.len(),
            playlist: Some(pid),
            warning: playlist.warning.clone(),
            sync: delta,
            local: rows.iter().filter(|row| row.is_ready()).count(),
            ..Default::default()
        };
        if sync.is_some() {
            self.library
                .sync_import_items(pid, &playlist.id, &playlist.name, &rows)
        } else {
            self.library.save_import_items(pid, &rows)
        }
        .map_err(|e| e.to_string())?;
        progress(ImportProgress::PlaylistReady(pid));

        // One job per recording; repeated playlist entries share download and
        // analysis, but keep separate rows. Four workers overlap network waits.
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut identities = std::collections::HashMap::new();
        for (index, t) in playlist.tracks.iter().enumerate() {
            if rows[index].is_ready() {
                continue;
            }
            let key = (&t.id, &t.title, &t.artist, t.duration_ms, &t.isrc);
            let group = *identities.entry(key).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            groups[group].push(index);
        }
        enum WorkerMsg {
            Stage(usize, &'static str),
            Done(usize, Result<(TrackId, bool), AudioFailure>),
        }
        let next = AtomicUsize::new(0);
        // Analyzer shares its CPU budget with library and AutoMix preparation;
        // downloads can keep progressing independently of those analysis slots.
        let result = std::thread::scope(|scope| -> Result<(), String> {
            let (tx, rx) = mpsc::channel();
            for _ in 0..groups.len().min(4) {
                let tx = tx.clone();
                let groups = &groups;
                let next = &next;
                let fetch = &fetch;
                let previous = &previous;
                let local = &local;
                scope.spawn(move || loop {
                    let group = next.fetch_add(1, Ordering::Relaxed);
                    let Some(indices) = groups.get(group) else {
                        break;
                    };
                    let t = &playlist.tracks[indices[0]];
                    let job = ResolveJob {
                        spotify_id: t.id.clone(),
                        title: t.title.clone(),
                        artist: t.artist.clone(),
                        duration_ms: t.duration_ms,
                        isrc: t.isrc.clone(),
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let previous = previous
                            .iter()
                            .find(|p| p.external_id == job.spotify_id && p.is_ready())
                            .and_then(|p| p.track_id);
                        self.recording(&job, previous, local, fetch, |status| {
                            let _ = tx.send(WorkerMsg::Stage(group, status));
                            Ok(())
                        })
                    }))
                    .unwrap_or_else(|_| {
                        Err("Import worker stopped unexpectedly".to_string().into())
                    });
                    let _ = tx.send(WorkerMsg::Done(group, result));
                });
            }
            drop(tx);
            let mut completed = rows.iter().filter(|row| row.is_ready()).count();
            for message in rx {
                let (group, status, id, error) = match message {
                    WorkerMsg::Stage(group, status) => (group, status, None, None),
                    WorkerMsg::Done(group, result) => {
                        let count = groups[group].len();
                        completed += count;
                        match result {
                            Ok((id, local)) => {
                                if local {
                                    report.local += count;
                                } else {
                                    report.acquired += count;
                                }
                                (
                                    group,
                                    if local { "local" } else { "acquired" },
                                    Some(id),
                                    None,
                                )
                            }
                            Err(error) => {
                                if error.suspect {
                                    report.suspect += count;
                                } else {
                                    report.failed += count;
                                }
                                let t = &playlist.tracks[groups[group][0]];
                                report
                                    .errors
                                    .push(format!("{} — {}: {}", t.artist, t.title, error.message));
                                (
                                    group,
                                    if error.suspect { "suspect" } else { "missing" },
                                    None,
                                    Some(error.message),
                                )
                            }
                        }
                    }
                };
                let updates: Vec<_> = groups[group]
                    .iter()
                    .map(|&index| {
                        let row = &mut rows[index];
                        row.status = status.into();
                        row.track_id = id;
                        row.error = error.clone();
                        row.clone()
                    })
                    .collect();
                self.library
                    .update_import_items(pid, &updates)
                    .map_err(|e| e.to_string())?;
                progress(ImportProgress::Changed(
                    pid,
                    format!("Importing · {completed} / {} complete", rows.len()),
                ));
            }
            Ok(())
        });
        if let Err(error) = result {
            for row in &mut rows {
                if row.is_pending() {
                    row.status = "missing".into();
                    row.error = Some(error.clone());
                }
            }
            let _ = self.library.update_import_items(pid, &rows);
            return Err(error);
        }
        Ok(report)
    }
}

fn snapshot_delta(previous: &[ImportItem], next: &[ImportItem]) -> (usize, usize) {
    let key = |row: &ImportItem| {
        if row.external_id.is_empty() {
            format!(
                "unavailable:{}:{}:{}",
                row.title, row.artist, row.duration_ms
            )
        } else {
            row.external_id.clone()
        }
    };
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for row in previous {
        *counts.entry(key(row)).or_default() += 1;
    }
    let mut added = 0;
    for row in next {
        let count = counts.entry(key(row)).or_default();
        if *count > 0 {
            *count -= 1;
        } else {
            added += 1;
        }
    }
    (added, counts.values().sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_spotify::SpotifyTrackMeta;
    #[test]
    fn spotify_sync_diffs_occurrences_preserves_audio_and_rejects_partial_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let path = dir.path().join("local.wav");
        let a = dir.path().join("a.wav");
        let b = dir.path().join("b.wav");
        for file in [&path, &a, &b] {
            wav(file, 1, 16);
        }
        let p = playlist(
            "remote",
            vec![
                remote('a', "Song A", 1),
                remote('b', "Song B", 1),
                remote('a', "Song A", 1),
            ],
        );
        let pid = service
            .spotify_playlist(
                &p,
                |job| {
                    Ok(if job.spotify_id.starts_with('a') {
                        a.clone()
                    } else {
                        b.clone()
                    })
                },
                |_| {},
            )
            .unwrap()
            .playlist
            .unwrap();
        let original = lib.import_items(pid).unwrap();
        let track = original[0].track_id.unwrap();
        let analysis = lib.load_analysis(track, ANALYSIS_VERSION).unwrap().unwrap();
        let mut next = playlist(
            "remote",
            vec![
                remote('b', "Song B", 1),
                remote('c', "Song C", 1),
                remote('a', "Song A", 1),
            ],
        );
        next.total_tracks = Some(3);
        next.warning = None;
        next.name = "Renamed".into();
        let report = service
            .sync_spotify_playlist(
                pid,
                &next,
                |job| {
                    assert_eq!(job.spotify_id, "c".repeat(22));
                    Ok(path.clone())
                },
                |event| {
                    if matches!(event, ImportProgress::PlaylistReady(_)) {
                        let rows = lib.import_items(pid).unwrap();
                        assert!(rows[0].is_ready());
                        assert!(rows[1].is_pending());
                        assert!(rows[2].is_ready());
                    }
                },
            )
            .unwrap();
        assert_eq!(report.sync, Some((1, 1)));
        assert_eq!(
            (report.local, report.acquired, report.failed, report.suspect),
            (2, 1, 0, 0)
        );
        assert!(report.summary().contains("1 added · 1 removed"));
        assert_eq!(
            lib.import_items(pid)
                .unwrap()
                .iter()
                .map(|r| r.external_id.chars().next().unwrap())
                .collect::<Vec<_>>(),
            ['b', 'c', 'a']
        );
        assert!(path.is_file());
        assert_eq!(
            lib.load_analysis(track, ANALYSIS_VERSION)
                .unwrap()
                .unwrap()
                .track_id,
            analysis.track_id
        );
        let snapshot = lib.import_items(pid).unwrap();
        next.total_tracks = Some(99);
        assert!(service
            .sync_spotify_playlist(pid, &next, |_| panic!("no download"), |_| {})
            .is_err());
        assert_eq!(lib.import_items(pid).unwrap(), snapshot);
        next.total_tracks = Some(0);
        next.tracks.clear();
        let report = service
            .sync_spotify_playlist(pid, &next, |_| panic!("empty"), |_| {})
            .unwrap();
        assert_eq!(report.sync, Some((0, 3)));
        assert!(lib.import_items(pid).unwrap().is_empty());
        assert!(lib.playlist_tracks(pid).unwrap().is_empty());
        assert!(lib.get_track(track).is_ok());
        assert!(path.is_file());
        // A deleted or mismatched target must never be recreated by sync.
        assert!(service
            .sync_spotify_playlist(PlaylistId(99999), &next, |_| panic!("deleted"), |_| {})
            .is_err());
    }

    #[test]
    #[ignore = "requires ffmpeg; exercises all file-picker formats through import and deck load"]
    fn local_formats_load_after_import() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let input = dir.path().join("source.wav");
        wav(&input, 2, 24);
        let engine = mixless_engine::Engine::new(mixless_engine::EngineConfig {
            offline: true,
            ..Default::default()
        })
        .unwrap();
        for (extension, codec) in [
            ("wav", "pcm_s16le"),
            ("mp3", "libmp3lame"),
            ("flac", "flac"),
            ("m4a", "aac"),
            ("aac", "aac"),
            ("ogg", "vorbis"),
            ("aiff", "pcm_s24be"),
        ] {
            let path = dir.path().join(format!("recording.{extension}"));
            let output = std::process::Command::new("ffmpeg")
                .args(["-v", "error", "-nostdin", "-i"])
                .arg(&input)
                .args([
                    "-c:a",
                    codec,
                    "-strict",
                    "experimental",
                    "-metadata",
                    "title=Recording",
                    "-metadata",
                    "artist=Artist",
                ])
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{extension}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let id = service
                .import_local_file(&path)
                .unwrap_or_else(|e| panic!("{extension}: {e}"));
            let track = lib.get_track(id).unwrap();
            assert!(
                track.analyzed && track.duration_ms.abs_diff(2000) < 150,
                "{extension}: {} ms",
                track.duration_ms
            );
            engine
                .load_file(
                    mixless_protocol::DeckId::A,
                    id,
                    &path,
                    track.title,
                    track.artist,
                )
                .unwrap();
            engine
                .dispatch(mixless_protocol::Command::PlayPause {
                    deck: mixless_protocol::DeckId::A,
                })
                .unwrap();
            engine
                .dispatch(mixless_protocol::Command::SetCrossfader { value: -1. })
                .unwrap();
            let rendered = engine.render_offline(4800);
            assert!(
                rendered.iter().all(|s| s.is_finite()) && rendered.iter().any(|s| s.abs() > 0.01),
                "{extension}: silent deck"
            );
            eprintln!("{extension}: import, analysis cache and deck playback passed");
        }
    }
    pub(super) fn wav(path: &Path, seconds: u32, bits: u16) {
        let mut w = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 22050,
                bits_per_sample: bits,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for i in 0..22050 * seconds {
            let sample = ((i as f32 / 22050. * 220. * std::f32::consts::TAU).sin()
                * 0.2
                * ((1u32 << (bits - 1)) - 1) as f32) as i32;
            w.write_sample(sample).unwrap();
            w.write_sample(sample).unwrap();
        }
        w.finalize().unwrap();
    }
    pub(super) fn remote(id: char, title: &str, seconds: u32) -> SpotifyTrackMeta {
        SpotifyTrackMeta {
            id: id.to_string().repeat(22),
            title: title.into(),
            artist: "Artist".into(),
            duration_ms: seconds * 1000,
            isrc: None,
        }
    }
    pub(super) fn playlist(id: &str, tracks: Vec<SpotifyTrackMeta>) -> SpotifyPlaylistMeta {
        SpotifyPlaylistMeta {
            id: id.into(),
            name: "Same display name".into(),
            owner: "Owner".into(),
            tracks,
            total_tracks: None,
            warning: Some("Public preview".into()),
        }
    }
    #[test]
    fn spotify_downloads_use_playlist_folder_and_reimports_reuse_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Downloads");
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let mut playlist = playlist("folders", vec![remote('a', "A", 2), remote('b', "B", 3)]);
        playlist.name = "夜行 / Drum & Bass".into();
        let dest = crate::playlist_download_dir(&root, &playlist.name);
        let report = service
            .spotify_playlist(
                &playlist,
                |job| {
                    std::fs::create_dir_all(&dest).unwrap();
                    let path = dest.join(format!("{}.wav", job.spotify_id));
                    wav(&path, job.duration_ms / 1000, 16);
                    Ok(path)
                },
                |_| {},
            )
            .unwrap();
        assert_eq!((report.acquired, report.failed), (2, 0));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let tracks = lib.playlist_tracks(report.playlist.unwrap()).unwrap();
        assert_eq!(tracks.len(), 2);
        for track in tracks {
            assert_eq!(
                Path::new(&track.path).parent(),
                Some(dest.canonicalize().unwrap().as_path())
            );
        }
        let retry = service
            .spotify_playlist(
                &playlist,
                |_| panic!("existing playlist audio must be reused"),
                |_| {},
            )
            .unwrap();
        assert_eq!((retry.local, retry.acquired, retry.failed), (2, 0, 0));
    }

    #[test]
    fn spotify_publishes_all_rows_before_parallel_work_and_deduplicates_downloads() {
        use std::sync::atomic::AtomicUsize;
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let p = playlist(
            "parallel",
            vec![
                remote('a', "A", 8),
                remote('b', "B", 8),
                remote('a', "A", 8),
            ],
        );
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let calls = AtomicUsize::new(0);
        let published = std::sync::atomic::AtomicBool::new(false);
        let result = service
            .spotify_playlist(
                &p,
                |_| {
                    assert!(published.load(Ordering::Acquire));
                    calls.fetch_add(1, Ordering::Relaxed);
                    let running = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(running, Ordering::SeqCst);
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                    while peak.load(Ordering::SeqCst) < 2 && std::time::Instant::now() < deadline {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    Err(AcquireError::Msg("fixture failure".into()))
                },
                |event| {
                    if let ImportProgress::PlaylistReady(pid) = event {
                        assert_eq!(lib.import_items(pid).unwrap().len(), 3);
                        assert!(lib.playlist_tracks(pid).unwrap().is_empty());
                        published.store(true, Ordering::Release);
                    }
                },
            )
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(peak.load(Ordering::SeqCst), 2);
        assert_eq!(result.failed, 3);
        let rows = lib.import_items(result.playlist.unwrap()).unwrap();
        assert!(rows
            .iter()
            .all(|r| r.status == "missing" && r.track_id.is_none()));
    }

    #[test]
    fn recursive_local_import_keeps_folder_playlists_separate_and_reimports_incrementally() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Music");
        let first = root.join("Set");
        let nested = first.join("Nested");
        let other = root.join("Other/Set");
        for folder in [&nested, &other] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let a = first.join("a.wav");
        let b = first.join("b.wav");
        let c = nested.join("c.wav");
        let d = other.join("d.wav");
        let e = root.join("e.wav");
        for path in [&a, &b, &c, &d, &e] {
            wav(path, 1, 16);
        }
        std::fs::write(root.join("cover.jpg"), "image").unwrap();
        std::fs::create_dir(root.join("Broken")).unwrap();
        std::fs::write(root.join("Broken/bad.mp3"), "not audio").unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let (paths, errors) =
            crate::local_paths::collect_audio(&[root.clone(), first.clone(), a.clone()]);
        assert!(errors.is_empty());
        let report = service.local_files(&paths, |_| {});
        assert_eq!((report.total, report.local, report.failed), (6, 5, 1));
        let playlists = lib.list_playlists().unwrap();
        assert_eq!(playlists.len(), 4);
        let mut first_playlist = None;
        for (folder, expected) in [
            (&root, vec![&e]),
            (&first, vec![&a, &b]),
            (&nested, vec![&c]),
            (&other, vec![&d]),
        ] {
            let folder_path = folder.canonicalize().unwrap();
            let playlist = playlists
                .iter()
                .find(|p| p.folder_path.as_deref() == folder_path.to_str())
                .unwrap();
            let mut actual: Vec<_> = lib
                .playlist_tracks(PlaylistId(playlist.id))
                .unwrap()
                .iter()
                .map(|t| PathBuf::from(&t.path))
                .collect();
            actual.sort();
            assert_eq!(
                actual,
                expected
                    .into_iter()
                    .map(|p| p.canonicalize().unwrap())
                    .collect::<Vec<_>>()
            );
            if folder == &first {
                first_playlist = Some(PlaylistId(playlist.id));
            }
        }
        let selected = report.playlist.unwrap();
        let first_ready = paths
            .iter()
            .find(|path| path.extension().unwrap() == "wav")
            .unwrap();
        assert!(lib
            .playlist_tracks(selected)
            .unwrap()
            .iter()
            .all(|t| Path::new(&t.path).parent() == first_ready.parent()));

        let new = first.join("new.wav");
        wav(&new, 1, 16);
        let retry = service.local_files(&[first.join("./a.wav"), new.clone(), a.clone()], |_| {});
        assert_eq!(retry.playlist, first_playlist);
        assert_eq!(retry.failed, 0);
        assert_eq!(lib.list_playlists().unwrap().len(), 4);
        let tracks = lib.playlist_tracks(first_playlist.unwrap()).unwrap();
        assert_eq!(tracks.len(), 3);
        assert_eq!(
            tracks
                .iter()
                .map(|t| PathBuf::from(&t.path))
                .collect::<Vec<_>>(),
            [&a, &b, &new]
                .into_iter()
                .map(|p| p.canonicalize().unwrap())
                .collect::<Vec<_>>()
        );
        #[cfg(unix)]
        {
            let alias = root.join("Alias");
            std::os::unix::fs::symlink(&first, &alias).unwrap();
            let retry = service.local_files(&[alias.join("a.wav")], |_| {});
            assert_eq!(retry.playlist, first_playlist);
            assert_eq!(
                lib.playlist_tracks(first_playlist.unwrap()).unwrap().len(),
                3
            );
        }
    }
    #[test]
    fn local_import_decodes_24_bit_reuses_canonical_path_and_reports_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let path = dir.path().join("音楽.wav");
        wav(&path, 1, 24);
        let bad = dir.path().join("broken.mp3");
        std::fs::write(&bad, "not audio").unwrap();
        let report = service.local_files(&[path.clone(), bad], |_| {});
        assert_eq!(report.local, 1);
        assert_eq!(report.failed, 1);
        assert_eq!(report.errors.len(), 1);
        let first = lib.list_tracks().unwrap().remove(0);
        assert!(first.analyzed);
        assert_eq!(first.duration_ms, 1000);
        assert!(lib
            .load_analysis(first.id, ANALYSIS_VERSION)
            .unwrap()
            .is_some());
        let second = service
            .import_local_file(&dir.path().join("./音楽.wav"))
            .unwrap();
        assert_eq!(second, first.id);
        assert_eq!(lib.list_tracks().unwrap().len(), 1);
        let audio = mixless_engine::decode_file(&path).unwrap();
        assert_eq!(audio.sample_rate, 22050);
        assert!(audio.samples.iter().any(|s| s.abs() > 0.1));
        wav(&path, 2, 24);
        let raw = lib.import_file(&path).unwrap();
        assert!(!lib.get_track(raw).unwrap().analyzed);
        assert!(lib.get_track(raw).unwrap().bpm.is_none());
        assert!(lib.load_analysis(raw, ANALYSIS_VERSION).unwrap().is_none());
        assert_eq!(service.import_local_file(&path).unwrap(), first.id);
        assert_eq!(lib.get_track(first.id).unwrap().duration_ms, 2000);
    }
    #[test]
    fn spotify_local_matches_need_no_sidecar_and_preserve_order_duplicates_and_identity() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let a = dir.path().join("a.wav");
        let b = dir.path().join("b.wav");
        wav(&a, 1, 16);
        wav(&b, 1, 16);
        let a = service.import_local_file(&a).unwrap();
        let b = service.import_local_file(&b).unwrap();
        lib.set_recording_metadata(a, "Song A", "Artist", None)
            .unwrap();
        lib.set_recording_metadata(b, "Song B", "Artist", None)
            .unwrap();
        let p = playlist(
            "remote-one",
            vec![
                remote('a', "Song A", 1),
                remote('b', "Song B", 1),
                remote('a', "Song A", 1),
            ],
        );
        let report = service
            .spotify_playlist(
                &p,
                |_| panic!("matching local files must not call provider"),
                |_| {},
            )
            .unwrap();
        let pid = report.playlist.unwrap();
        assert_eq!(report.local, 3);
        assert_eq!(
            lib.playlist_tracks(pid)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            [a, b, a]
        );
        let other = service
            .spotify_playlist(
                &playlist("remote-two", vec![remote('b', "Song B", 1)]),
                |_| panic!("no provider"),
                |_| {},
            )
            .unwrap();
        assert_ne!(pid, other.playlist.unwrap());
        assert_eq!(lib.list_playlists().unwrap().len(), 3);
        let again = service
            .spotify_playlist(
                &p,
                |_| panic!("repeat import should reuse local audio"),
                |_| {},
            )
            .unwrap();
        assert_eq!(again.playlist, Some(pid));
    }
    #[test]
    fn isrc_reuses_a_local_file_even_when_title_and_artist_differ() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let path = dir.path().join("local.wav");
        wav(&path, 1, 16);
        let id = service.import_local_file(&path).unwrap();
        lib.set_recording_metadata(id, "Local Name", "Local Artist", Some("USXJA1"))
            .unwrap();
        let mut meta = remote('a', "Other Name", 1);
        meta.isrc = Some("usxja1".into());
        let report = service
            .spotify_playlist(
                &playlist("remote", vec![meta]),
                |_| panic!("isrc match must not call provider"),
                |_| {},
            )
            .unwrap();
        assert_eq!(report.local, 1);
        assert_eq!(
            lib.playlist_tracks(report.playlist.unwrap()).unwrap()[0].id,
            id
        );
    }
    #[test]
    fn missing_suspect_and_failed_downloads_cannot_enter_automix_and_can_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let valid = dir.path().join("valid.wav");
        let wrong = dir.path().join("wrong.wav");
        wav(&valid, 8, 16);
        wav(&wrong, 1, 16);
        let p = playlist(
            "remote",
            vec![
                remote('a', "Acquired", 8),
                remote('b', "Wrong version", 8),
                remote('c', "Unavailable", 8),
            ],
        );
        let report = service
            .spotify_playlist(
                &p,
                |job| match job.spotify_id.as_bytes()[0] {
                    b'a' => Ok(valid.clone()),
                    b'b' => Ok(wrong.clone()),
                    _ => Err(AcquireError::Msg("source unavailable".into())),
                },
                |_| {},
            )
            .unwrap();
        let pid = report.playlist.unwrap();
        assert_eq!((report.acquired, report.suspect, report.failed), (1, 1, 1));
        assert_eq!(lib.list_tracks().unwrap().len(), 1);
        assert_eq!(lib.playlist_tracks(pid).unwrap().len(), 1);
        let rows = lib.import_items(pid).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.status.as_str()).collect::<Vec<_>>(),
            ["acquired", "suspect", "missing"]
        );
        assert!(rows[1].track_id.is_none());
        assert!(rows[2].error.as_ref().unwrap().contains("unavailable"));
        let fixed = dir.path().join("fixed.wav");
        wav(&fixed, 8, 16);
        let retry = service
            .spotify_playlist(&p, |_| Ok(fixed.clone()), |_| {})
            .unwrap();
        assert_eq!(retry.local + retry.acquired, 3);
        assert_eq!(lib.playlist_tracks(pid).unwrap().len(), 3);
    }
    #[test]
    fn rejected_provider_files_are_removed_so_retries_fetch_a_new_source() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        // Provider naming: `<spotify id>.<ext>` plus a `.json` provenance file.
        let dest = dir.path().join("downloads");
        std::fs::create_dir_all(&dest).unwrap();
        let id = "b".repeat(22);
        let fetched = dest.join(format!("{id}.wav"));
        let sidecar = dest.join(format!("{id}.json"));
        wav(&fetched, 1, 16);
        std::fs::write(&sidecar, "{}").unwrap();
        let p = playlist("remote", vec![remote('b', "Wrong Version", 8)]);
        let report = service
            .spotify_playlist(&p, |_| Ok(fetched.clone()), |_| {})
            .unwrap();
        assert_eq!(report.suspect, 1);
        assert!(!fetched.exists() && !sidecar.exists());
        wav(&fetched, 8, 16);
        let retry = service
            .spotify_playlist(&p, |_| Ok(fetched.clone()), |_| {})
            .unwrap();
        assert_eq!(retry.acquired, 1);
    }
    #[test]
    fn invalid_playlist_replacement_rolls_back_existing_rows() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("lib.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &lib,
            analyzer: &analyzer,
        };
        let path = dir.path().join("a.wav");
        wav(&path, 1, 16);
        let id = service.import_local_file(&path).unwrap();
        let pid = lib.replace_playlist("Local", &[id]).unwrap();
        assert!(lib.replace_playlist("Local", &[TrackId(999999)]).is_err());
        assert_eq!(lib.playlist_tracks(pid).unwrap()[0].id, id);
        let remote = lib.external_playlist("spotify", "remote", "Local").unwrap();
        assert_ne!(pid, remote);
        let good = ImportItem {
            position: 0,
            external_id: "a".repeat(22),
            title: "Song".into(),
            artist: "Artist".into(),
            duration_ms: 1000,
            status: "local".into(),
            track_id: Some(id),
            error: None,
        };
        lib.save_import_items(remote, &[good.clone()]).unwrap();
        let mut broken = good;
        broken.track_id = Some(TrackId(999999));
        assert!(lib.save_import_items(remote, &[broken]).is_err());
        assert_eq!(lib.playlist_tracks(remote).unwrap()[0].id, id);
    }
}
