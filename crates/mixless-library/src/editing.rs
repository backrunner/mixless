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
