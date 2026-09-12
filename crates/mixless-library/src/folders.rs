//! Local playlists are keyed by the full parent path stored on each track.
//! Files are canonicalized at import, so aliases share a playlist while equal
//! folder names at different locations remain independent.
use super::*;

impl Library {
    /// Register the directory before scanning or analyzing its contents.
    pub fn register_folder(&self, path: &Path) -> Result<PlaylistId, LibraryError> {
        let path = path.canonicalize()?;
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        self.external_playlist("folder", &path.to_string_lossy(), &name)
    }

    /// Fast discovery pass: filenames and membership only, no audio reads.
    /// A pending row stays visible even when decoding or analysis later fails.
    pub fn register_local_files(&self, paths: &[PathBuf]) -> Result<Vec<TrackId>, LibraryError> {
        self.register_local_files_into(paths, &[])
    }

    pub fn register_local_files_into(
        &self,
        paths: &[PathBuf],
        roots: &[PathBuf],
    ) -> Result<Vec<TrackId>, LibraryError> {
        let paths = paths
            .iter()
            .map(fs::canonicalize)
            .collect::<Result<Vec<_>, _>>()?;
        let roots = roots
            .iter()
            .map(fs::canonicalize)
            .collect::<Result<Vec<_>, _>>()?;
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let mut ids = Vec::with_capacity(paths.len());
        for path in &paths {
            let title = path
                .file_stem()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            tx.execute(
                "INSERT INTO tracks(path,title,artist,duration_ms,content_hash,mtime)
                 VALUES (?1,?2,'',0,'',0) ON CONFLICT(path) DO NOTHING",
                params![path.to_string_lossy(), title],
            )?;
            let id = TrackId(tx.query_row(
                "SELECT id FROM tracks WHERE path=?1",
                [path.to_string_lossy()],
                |row| row.get(0),
            )?);
            add_folder_track(&tx, id, path)?;
            for root in &roots {
                if path.starts_with(root) {
                    add_folder_member(&tx, id, root)?;
                }
            }
            ids.push(id);
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Append a ready local track without replacing previously imported siblings.
    pub fn add_to_folder_playlist(&self, track: TrackId) -> Result<PlaylistId, LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let path: String = tx.query_row(
            "SELECT path FROM tracks WHERE id=?1 AND analyzed=1",
            [track.0],
            |r| r.get(0),
        )?;
        let playlist = add_folder_track(&tx, track, Path::new(&path))?;
        tx.commit()?;
        Ok(playlist)
    }

    /// Upgrade existing local tracks using saved paths, including offline disks.
    /// Managed downloads keep their remote playlist membership. This is additive
    /// and idempotent; no remote or manually created playlist is rewritten.
    pub(super) fn backfill_folder_playlists(&self, acquired: &Path) -> Result<(), LibraryError> {
        let acquired = acquired
            .canonicalize()
            .unwrap_or_else(|_| acquired.to_owned());
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let tracks = {
            let mut stmt = tx.prepare(
                "SELECT t.id,t.path FROM tracks t WHERE t.analyzed=1
                 AND NOT EXISTS (
                     SELECT 1 FROM playlist_items i
                     JOIN external_playlists e ON e.playlist_id=i.playlist_id
                     WHERE i.track_id=t.id AND e.source='folder')
                 AND NOT EXISTS (
                     SELECT 1 FROM import_items i
                     WHERE i.track_id=t.id AND i.status='acquired')
                 ORDER BY t.path",
            )?;
            let rows = stmt.query_map([], |r| Ok((TrackId(r.get(0)?), r.get::<_, String>(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (track, path) in tracks {
            let path = Path::new(&path);
            if !path.starts_with(&acquired) {
                add_folder_track(&tx, track, path)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn add_folder_track(
    conn: &Connection,
    track: TrackId,
    path: &Path,
) -> Result<PlaylistId, LibraryError> {
    add_folder_member(conn, track, path.parent().ok_or(LibraryError::NotFound)?)
}

fn add_folder_member(
    conn: &Connection,
    track: TrackId,
    folder: &Path,
) -> Result<PlaylistId, LibraryError> {
    let folder_path = folder.to_string_lossy();
    let existing: Option<i64> = conn
        .query_row(
            "SELECT playlist_id FROM external_playlists WHERE source='folder' AND external_id=?1",
            [folder_path.as_ref()],
            |r| r.get(0),
        )
        .optional()?;
    let id = if let Some(id) = existing {
        id
    } else {
        let name = folder
            .file_name()
            .unwrap_or(folder.as_os_str())
            .to_string_lossy();
        conn.execute("INSERT INTO playlists(name) VALUES (?1)", [name.as_ref()])?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO external_playlists(source,external_id,playlist_id) VALUES ('folder',?1,?2)",
            params![folder_path, id],
        )?;
        id
    };
    conn.execute(
        "INSERT INTO playlist_items(playlist_id,position,track_id)
         SELECT ?1,COALESCE((SELECT MAX(position)+1 FROM playlist_items WHERE playlist_id=?1),0),?2
         WHERE NOT EXISTS (SELECT 1 FROM playlist_items WHERE playlist_id=?1 AND track_id=?2)
         AND NOT EXISTS (SELECT 1 FROM playlist_exclusions WHERE playlist_id=?1 AND track_id=?2)",
        params![id, track.0],
    )?;
    Ok(PlaylistId(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved_track(library: &Library, path: &Path, analyzed: bool) -> TrackId {
        let conn = library.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tracks(path,title,artist,duration_ms,content_hash,mtime,analyzed)
             VALUES (?1,'Song','Artist',1000,'fixture',0,?2)",
            params![path.to_string_lossy(), analyzed],
        )
        .unwrap();
        TrackId(conn.last_insert_rowid())
    }

    #[test]
    fn reopening_groups_offline_local_tracks_without_changing_remote_playlists() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("library.db");
        let library = Library::open(&database).unwrap();
        let a = saved_track(&library, &dir.path().join("offline/Set/a.wav"), true);
        let b = saved_track(&library, &dir.path().join("other/Set/b.wav"), true);
        let child = saved_track(&library, &dir.path().join("offline/Set/Sub/c.wav"), true);
        let download = saved_track(&library, &dir.path().join("downloaded/d.wav"), true);
        saved_track(&library, &dir.path().join("acquired/managed.wav"), true);
        saved_track(&library, &dir.path().join("broken/bad.wav"), false);
        let remote = library
            .external_playlist("spotify", "remote", "Set")
            .unwrap();
        let item = |position, track_id, status: &str| ImportItem {
            position,
            external_id: format!("remote-{position}"),
            title: "Song".into(),
            artist: "Artist".into(),
            duration_ms: 1000,
            status: status.into(),
            track_id: Some(track_id),
            error: None,
        };
        library
            .save_import_items(
                remote,
                &[
                    item(0, a, "local"),
                    item(1, download, "acquired"),
                    item(2, a, "local"),
                ],
            )
            .unwrap();
        drop(library);

        let library = Library::open(&database).unwrap();
        let playlists = library.list_playlists().unwrap();
        assert_eq!(playlists.len(), 4);
        for (suffix, track) in [
            ("offline/Set", a),
            ("other/Set", b),
            ("offline/Set/Sub", child),
        ] {
            let playlist = playlists
                .iter()
                .find(|p| {
                    p.folder_path
                        .as_ref()
                        .is_some_and(|path| Path::new(path) == dir.path().join(suffix))
                })
                .unwrap();
            assert_eq!(playlist.tracks, 1);
            assert_eq!(
                library.playlist_tracks(PlaylistId(playlist.id)).unwrap()[0].id,
                track
            );
        }
        assert_eq!(
            library
                .playlist_tracks(remote)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            [a, download, a]
        );
        assert_eq!(library.import_items(remote).unwrap().len(), 3);
        let ids: Vec<_> = playlists.iter().map(|p| p.id).collect();
        drop(library);
        let library = Library::open(&database).unwrap();
        assert_eq!(
            library
                .list_playlists()
                .unwrap()
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>(),
            ids
        );
    }

    #[test]
    fn invalid_track_does_not_create_an_empty_folder_playlist() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(&dir.path().join("library.db")).unwrap();
        let unready = saved_track(&library, &dir.path().join("Set/bad.wav"), false);
        assert!(library.add_to_folder_playlist(unready).is_err());
        assert!(library.add_to_folder_playlist(TrackId(999)).is_err());
        assert!(library.list_playlists().unwrap().is_empty());
    }
}
