use super::*;

impl ImportService<'_> {
    /// Retry the persisted recording in its original slot, without fetching or
    /// replacing the remote playlist. `fetch` receives the current playlist name.
    pub fn retry_spotify_item(
        &self,
        playlist: PlaylistId,
        position: usize,
        external_id: &str,
        fetch: impl Fn(&ResolveJob, &str) -> Result<PathBuf, AcquireError>,
        mut progress: impl FnMut(ImportProgress),
    ) -> Result<ImportReport, String> {
        let (name, mut row) = self
            .library
            .begin_import_retry(playlist, position, external_id)
            .map_err(|e| e.to_string())?
            .ok_or("This track was removed, already imported, or is already being retried")?;
        progress(ImportProgress::Changed(
            playlist,
            format!("Retrying · {}", row.title),
        ));
        let job = ResolveJob {
            spotify_id: row.external_id.clone(),
            title: row.title.clone(),
            artist: row.artist.clone(),
            duration_ms: row.duration_ms,
            isrc: None,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let local = recording::LocalRecordings::read(self.library)?;
            self.recording(&job, None, &local, &|job| fetch(job, &name), |status| {
                row.status = status.into();
                self.library
                    .update_import_items(playlist, std::slice::from_ref(&row))
                    .map_err(|e| e.to_string())?;
                progress(ImportProgress::Changed(
                    playlist,
                    format!("Retrying · {} · {status}", row.title),
                ));
                Ok(())
            })
        }))
        .unwrap_or_else(|_| Err("Import worker stopped unexpectedly".to_string().into()));
        let mut report = ImportReport {
            total: 1,
            playlist: Some(playlist),
            ..Default::default()
        };
        match result {
            Ok((id, local)) => {
                row.status = if local { "local" } else { "acquired" }.into();
                row.track_id = Some(id);
                if local {
                    report.local = 1;
                } else {
                    report.acquired = 1;
                }
            }
            Err(error) => {
                row.status = if error.suspect { "suspect" } else { "missing" }.into();
                row.error = Some(error.message.clone());
                if error.suspect {
                    report.suspect = 1;
                } else {
                    report.failed = 1;
                }
                report
                    .errors
                    .push(format!("{} — {}: {}", row.artist, row.title, error.message));
            }
        }
        self.library
            .update_import_items(playlist, &[row])
            .map_err(|e| e.to_string())?;
        progress(ImportProgress::Changed(playlist, report.summary()));
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports::tests::{playlist, remote, wav};

    #[test]
    fn retry_only_changes_selected_row_and_preserves_order_removals_and_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(&dir.path().join("library.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &library,
            analyzer: &analyzer,
        };
        let remote = playlist(
            "retry",
            vec![
                remote('a', "A", 2),
                remote('b', "B", 3),
                remote('c', "C", 4),
                remote('b', "B", 3),
                remote('d', "D", 5),
            ],
        );
        let report = service
            .spotify_playlist(
                &remote,
                |job| {
                    if job.title == "B" || job.title == "D" {
                        return Err(AcquireError::Msg("offline".into()));
                    }
                    let path = dir.path().join(format!("{}.wav", job.spotify_id));
                    wav(&path, job.duration_ms / 1000, 16);
                    Ok(path)
                },
                |_| {},
            )
            .unwrap();
        let pid = report.playlist.unwrap();
        let ready: Vec<_> = library
            .playlist_tracks(pid)
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect();
        library
            .reorder_tracks(Some(pid), &ready, &[ready[1], ready[0]])
            .unwrap();
        library.remove_import_item(pid, 4).unwrap();
        library
            .external_playlist("spotify", "retry", "夜行 / Renamed")
            .unwrap();
        let before = library.import_items(pid).unwrap();
        let mut stages = Vec::new();
        let report = service
            .retry_spotify_item(
                pid,
                1,
                &"b".repeat(22),
                |job, name| {
                    assert_eq!(job.title, "B");
                    assert_eq!(name, "夜行 / Renamed");
                    let dest = crate::playlist_download_dir(&dir.path().join("Downloads"), name);
                    std::fs::create_dir_all(&dest).unwrap();
                    let path = dest.join(format!("{}.wav", job.spotify_id));
                    wav(&path, 3, 16);
                    Ok(path)
                },
                |_| {
                    stages.push(library.import_items(pid).unwrap()[1].status.clone());
                },
            )
            .unwrap();
        assert_eq!((report.total, report.acquired, report.failed), (1, 1, 0));
        assert_eq!(stages, ["queued", "resolving", "analyzing", "acquired"]);
        let after = library.import_items(pid).unwrap();
        assert_eq!(after.len(), before.len());
        for index in [0, 2, 3] {
            assert_eq!(before[index], after[index]);
        }
        let retried = after[1].track_id.unwrap();
        assert_eq!(
            library
                .playlist_tracks(pid)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            [ready[1], retried, ready[0]]
        );
        let report = service
            .retry_spotify_item(
                pid,
                3,
                &"b".repeat(22),
                |_, _| panic!("another occurrence should reuse the local recording"),
                |_| {},
            )
            .unwrap();
        assert_eq!((report.local, report.acquired, report.failed), (1, 0, 0));
        assert_eq!(
            library
                .playlist_tracks(pid)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            [ready[1], retried, ready[0], retried]
        );
        assert!(service
            .retry_spotify_item(
                pid,
                1,
                &"b".repeat(22),
                |_, _| panic!("already ready"),
                |_| {}
            )
            .is_err());
    }

    #[test]
    fn retry_failures_keep_the_new_error_and_can_be_retried_again() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(&dir.path().join("library.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &library,
            analyzer: &analyzer,
        };
        let playlist = playlist("retry", vec![remote('a', "A", 2)]);
        let pid = service
            .spotify_playlist(
                &playlist,
                |_| Err(AcquireError::Msg("offline".into())),
                |_| {},
            )
            .unwrap()
            .playlist
            .unwrap();
        let id = "a".repeat(22);
        let failed = service
            .retry_spotify_item(
                pid,
                0,
                &id,
                |_, _| Err(AcquireError::Msg("still unavailable".into())),
                |_| {},
            )
            .unwrap();
        assert_eq!(failed.failed, 1);
        let row = library.import_items(pid).unwrap().remove(0);
        assert!(row.is_failed());
        assert_eq!(row.error.as_deref(), Some("still unavailable"));
        let path = dir.path().join(format!("{id}.wav"));
        let suspect = service
            .retry_spotify_item(
                pid,
                0,
                &id,
                |_, _| {
                    wav(&path, 10, 16);
                    Ok(path.clone())
                },
                |_| {},
            )
            .unwrap();
        assert_eq!(suspect.suspect, 1);
        assert!(!path.exists());
        assert_eq!(library.import_items(pid).unwrap()[0].status, "suspect");
        let panicked = service
            .retry_spotify_item(pid, 0, &id, |_, _| panic!("fixture failure"), |_| {})
            .unwrap();
        assert_eq!(panicked.failed, 1);
        assert!(library.import_items(pid).unwrap()[0].is_failed());
        assert!(library.playlist_tracks(pid).unwrap().is_empty());
    }

    #[test]
    fn stale_or_duplicate_retry_cannot_download_and_removal_stays_final() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(&dir.path().join("library.db")).unwrap();
        let analyzer = Analyzer::new();
        let service = ImportService {
            library: &library,
            analyzer: &analyzer,
        };
        let playlist = playlist("retry", vec![remote('a', "A", 2)]);
        let pid = service
            .spotify_playlist(
                &playlist,
                |_| Err(AcquireError::Msg("offline".into())),
                |_| {},
            )
            .unwrap()
            .playlist
            .unwrap();
        let id = "a".repeat(22);
        assert!(service
            .retry_spotify_item(
                pid,
                0,
                &"b".repeat(22),
                |_, _| panic!("stale identity"),
                |_| {}
            )
            .is_err());
        service
            .retry_spotify_item(
                pid,
                0,
                &id,
                |_, _| {
                    assert!(service
                        .retry_spotify_item(pid, 0, &id, |_, _| panic!("duplicate request"), |_| {})
                        .is_err());
                    library.remove_import_item(pid, 0).unwrap();
                    let path = dir.path().join("a.wav");
                    wav(&path, 2, 16);
                    Ok(path)
                },
                |_| {},
            )
            .unwrap();
        assert!(library.import_items(pid).unwrap().is_empty());
        assert!(library.playlist_tracks(pid).unwrap().is_empty());
        assert!(service
            .retry_spotify_item(pid, 0, &id, |_, _| panic!("removed row"), |_| {})
            .is_err());
        library.remove_playlist(pid).unwrap();
        assert!(service
            .retry_spotify_item(pid, 0, &id, |_, _| panic!("removed playlist"), |_| {})
            .is_err());
    }
}
