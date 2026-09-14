//! Membership and analysis edits never remove source audio.
use super::*;

impl Library {
    pub fn remove_playlist_track(
        &self,
        playlist: PlaylistId,
        track: TrackId,
    ) -> Result<(), LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM playlist_items WHERE playlist_id=?1 AND track_id=?2",
            params![playlist.0, track.0],
        )?;
        // Local-folder backfill must not undo an explicit membership edit.
        tx.execute(
            "INSERT OR IGNORE INTO playlist_exclusions(playlist_id,track_id) VALUES (?1,?2)",
            params![playlist.0, track.0],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Folder-backed copies become plain manual playlists; exclusions and
    /// unresolved import rows are not carried over.
    pub fn duplicate_playlist(&self, id: PlaylistId) -> Result<PlaylistId, LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let (name, folder): (String, Option<String>) = tx
            .query_row(
                "SELECT p.name,
                        (SELECT e.external_id FROM external_playlists e
                         WHERE e.playlist_id=p.id AND e.source='folder')
                 FROM playlists p WHERE p.id=?1",
                [id.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(LibraryError::NotFound)?;
        let display = folder
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(name);
        // The suffix only competes with manual playlists; the folder row the
        // copy was cloned from keeps its own name.
        let mut candidate = format!("{display} copy");
        for n in 2.. {
            let taken: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM playlists WHERE name=?1
                 AND id NOT IN (SELECT playlist_id FROM external_playlists))",
                [&candidate],
                |r| r.get(0),
            )?;
            if !taken {
                break;
            }
            candidate = format!("{display} copy {n}");
        }
        tx.execute("INSERT INTO playlists(name) VALUES (?1)", [&candidate])?;
        let copy = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO playlist_items(playlist_id,position,track_id)
             SELECT ?1,position,track_id FROM playlist_items WHERE playlist_id=?2",
            params![copy, id.0],
        )?;
        tx.commit()?;
        Ok(PlaylistId(copy))
    }

    /// Removing a folder playlist hides the path so later scans do not
    /// recreate it; `register_folder` lifts the hide. Tracks and audio stay.
    pub fn remove_playlist(&self, id: PlaylistId) -> Result<(), LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let folder: Option<String> = tx
            .query_row(
                "SELECT external_id FROM external_playlists
                 WHERE playlist_id=?1 AND source='folder'",
                [id.0],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(path) = folder {
            tx.execute(
                "INSERT OR IGNORE INTO hidden_folder_playlists(folder_path) VALUES (?1)",
                [path],
            )?;
        }
        if tx.execute("DELETE FROM playlists WHERE id=?1", [id.0])? != 1 {
            return Err(LibraryError::NotFound);
        }
        tx.commit()?;
        Ok(())
    }

    pub fn reset_analysis(&self, id: TrackId, clear_cues: bool) -> Result<(), LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        if tx.execute(
            "UPDATE tracks SET analyzed=0,bpm=NULL,key=NULL,camelot=NULL WHERE id=?1",
            [id.0],
        )? != 1
        {
            return Err(LibraryError::NotFound);
        }
        tx.execute("DELETE FROM track_analysis WHERE track_id=?1", [id.0])?;
        tx.execute(
            "DELETE FROM cues WHERE track_id=?1 AND (?2 OR user_set=0)",
            params![id.0, clear_cues],
        )?;
        tx.execute("DELETE FROM cue_versions WHERE track_id=?1", [id.0])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removal_survives_folder_backfill_without_deleting_other_membership_or_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wav");
        fs::write(&path, b"fixture").unwrap();
        let db = dir.path().join("library.db");
        let lib = Library::open(&db).unwrap();
        let id = lib.register_local_files(&[path.clone()]).unwrap()[0];
        let folder = PlaylistId(lib.list_playlists().unwrap()[0].id);
        let other = lib.replace_playlist("Another", &[id]).unwrap();
        lib.remove_playlist_track(folder, id).unwrap();
        drop(lib);
        let lib = Library::open(&db).unwrap();
        lib.register_local_files(&[path.clone()]).unwrap();
        assert!(lib.playlist_tracks(folder).unwrap().is_empty());
        assert_eq!(lib.playlist_tracks(other).unwrap()[0].id, id);
        assert!(lib.get_track(id).is_ok() && path.exists());
    }

    #[test]
    fn duplicate_copies_items_into_a_manual_playlist_with_a_numbered_copy_name() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Set");
        fs::create_dir_all(&folder).unwrap();
        let a = folder.join("a.wav");
        let b = folder.join("b.wav");
        fs::write(&a, b"fixture").unwrap();
        fs::write(&b, b"fixture").unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let ids = lib.register_local_files(&[a, b]).unwrap();
        let folder_playlist = lib
            .list_playlists()
            .unwrap()
            .into_iter()
            .find(|p| p.folder_path.is_some())
            .unwrap();
        let copy = lib
            .duplicate_playlist(PlaylistId(folder_playlist.id))
            .unwrap();
        let copy2 = lib
            .duplicate_playlist(PlaylistId(folder_playlist.id))
            .unwrap();
        let playlists = lib.list_playlists().unwrap();
        let by_id = |id: PlaylistId| playlists.iter().find(|p| p.id == id.0).unwrap();
        assert_eq!(by_id(copy).name, "Set copy");
        assert!(by_id(copy).folder_path.is_none());
        assert_eq!(by_id(copy2).name, "Set copy 2");
        assert_eq!(
            lib.playlist_tracks(copy)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            ids
        );
        // The source folder playlist and its exclusion machinery are untouched.
        assert_eq!(by_id(PlaylistId(folder_playlist.id)).tracks, 2);
    }

    #[test]
    fn reanalysis_can_preserve_manual_cues_or_explicitly_clear_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track.wav");
        fs::write(&path, b"fixture").unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let id = lib.import_file(&path).unwrap();
        lib.set_cue(id, 0, 200, CueKind::In, true).unwrap();
        lib.set_cue(id, 1, 300, CueKind::Out, false).unwrap();
        lib.reset_analysis(id, false).unwrap();
        assert_eq!(lib.cues(id).unwrap().len(), 1);
        assert!(lib.cues(id).unwrap()[0].user_set);
        lib.reset_analysis(id, true).unwrap();
        assert!(lib.cues(id).unwrap().is_empty());
        assert!(!lib.get_track(id).unwrap().analyzed);
    }
}
