//! Import orchestration runs on a host worker. A row is ready only after the
//! engine's decoder and offline analysis have accepted the actual local file.
use crate::{
    AcquireError, ResolveJob, could_match, duration_ok, job_key, local_match, track_key,
};
use mixless_analyze::{ANALYSIS_VERSION, Analyzer};
use mixless_library::{ImportItem, Library, content_hash};
use mixless_protocol::{PlaylistId, TrackId};
use mixless_spotify::SpotifyPlaylistMeta;
use std::path::{Path, PathBuf};

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
}
impl ImportReport {
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("Added {} tracks", self.local + self.acquired)];
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.suspect > 0 {
            parts.push(format!("{} need review", self.suspect));
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
            .list_tracks()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|t| Path::new(&t.path) == canon && t.content_hash == hash);
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
        mut fetch: impl FnMut(&ResolveJob) -> Result<PathBuf, AcquireError>,
        mut progress: impl FnMut(String),
    ) -> Result<ImportReport, String> {
        let pid = self
            .library
            .external_playlist("spotify", &playlist.id, &playlist.name)
            .map_err(|e| e.to_string())?;
        let previous = self.library.import_items(pid).map_err(|e| e.to_string())?;
        let mut tracks = self.library.list_tracks().map_err(|e| e.to_string())?;
        // Normalize each track's identity once up front; `could_match` then
        // skips non-candidates so `local_match` does not re-normalize the whole
        // library for every playlist row.
        let mut keys: Vec<_> = tracks.iter().map(track_key).collect();
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
        let mut report = ImportReport {
            total: rows.len(),
            playlist: Some(pid),
            warning: playlist.warning.clone(),
            ..Default::default()
        };
        self.library
            .save_import_items(pid, &rows)
            .map_err(|e| e.to_string())?;
        for index in 0..rows.len() {
            let t = &playlist.tracks[index];
            let job = ResolveJob {
                spotify_id: t.id.clone(),
                title: t.title.clone(),
                artist: t.artist.clone(),
                duration_ms: t.duration_ms,
                isrc: t.isrc.clone(),
            };
            progress(format!(
                "{} / {} · {} — {}",
                index + 1,
                rows.len(),
                t.artist,
                t.title
            ));
            let row = &mut rows[index];
            let result = (|| -> Result<(TrackId, bool), AudioFailure> {
                if job.spotify_id.len() != 22
                    || !job.spotify_id.bytes().all(|c| c.is_ascii_alphanumeric())
                    || job.duration_ms == 0
                {
                    return Err("unavailable track or missing recording metadata"
                        .to_string()
                        .into());
                }
                let linked = previous
                    .iter()
                    .find(|p| {
                        p.external_id == job.spotify_id
                            && matches!(p.status.as_str(), "local" | "acquired")
                    })
                    .and_then(|p| p.track_id)
                    .or(self
                        .library
                        .linked_import_track(&job.spotify_id)
                        .map_err(|e| e.to_string())?);
                let local = linked
                    .and_then(|id| {
                        tracks
                            .iter()
                            .find(|t| t.id == id && Path::new(&t.path).is_file())
                    })
                    .or_else(|| {
                        let want = job_key(&job);
                        tracks
                            .iter()
                            .zip(&keys)
                            .find(|&(t, key)| {
                                could_match(&want, key)
                                    && local_match(&job, t)
                                    && Path::new(&t.path).is_file()
                            })
                            .map(|(t, _)| t)
                    });
                if let Some(track) = local {
                    if let Ok(id) = self.audio(Path::new(&track.path), Some(job.duration_ms)) {
                        return Ok((id, true));
                    }
                    // Missing/stale/corrupt matches may be replaced by the provider.
                }
                let path = fetch(&job).map_err(|e| e.to_string())?;
                let id = match self.audio(&path, Some(job.duration_ms)) {
                    Ok(id) => id,
                    Err(error) => {
                        // Provider downloads are named `<spotify id>.<ext>`;
                        // only those are removed so a rejected download cannot
                        // satisfy the provider's reuse check on the next retry.
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
            })();
            match result {
                Ok((id, local)) => {
                    row.track_id = Some(id);
                    row.status = if local {
                        report.local += 1;
                        "local"
                    } else {
                        report.acquired += 1;
                        "acquired"
                    }
                    .into();
                    if let Ok(track) = self.library.get_track(id) {
                        if let Some(pos) = tracks.iter().position(|t| t.id == id) {
                            tracks.remove(pos);
                            keys.remove(pos);
                        }
                        keys.push(track_key(&track));
                        tracks.push(track);
                    }
                }
                Err(error) => {
                    row.status = if error.suspect {
                        report.suspect += 1;
                        "suspect"
                    } else {
                        report.failed += 1;
                        "missing"
                    }
                    .into();
                    report
                        .errors
                        .push(format!("{} — {}: {}", t.artist, t.title, error.message));
                    row.error = Some(error.message);
                }
            }
            progress(format!(
                "{} / {} · {} [{}]",
                index + 1,
                playlist.tracks.len(),
                t.title,
                row.status
            ));
            self.library
                .save_import_items(pid, &rows)
                .map_err(|e| e.to_string())?;
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_spotify::SpotifyTrackMeta;
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
    fn wav(path: &Path, seconds: u32, bits: u16) {
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
    fn remote(id: char, title: &str, seconds: u32) -> SpotifyTrackMeta {
        SpotifyTrackMeta {
            id: id.to_string().repeat(22),
            title: title.into(),
            artist: "Artist".into(),
            duration_ms: seconds * 1000,
            isrc: None,
        }
    }
    fn playlist(id: &str, tracks: Vec<SpotifyTrackMeta>) -> SpotifyPlaylistMeta {
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
        assert!(
            lib.playlist_tracks(selected)
                .unwrap()
                .iter()
                .all(|t| Path::new(&t.path).parent() == first_ready.parent())
        );

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
        assert!(
            lib.load_analysis(first.id, ANALYSIS_VERSION)
                .unwrap()
                .is_some()
        );
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
