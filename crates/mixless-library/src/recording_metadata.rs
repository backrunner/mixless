//! An external tag rewrite must not replace a known recording with its filename.
use super::*;

impl Library {
    pub(super) fn protect_import_metadata(
        &self,
        path: &Path,
        meta: &mut FileMeta,
    ) -> Result<(), LibraryError> {
        let fallback = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if meta.title != fallback && meta.artist != "Unknown" {
            return Ok(());
        }
        let known: Option<(String, String)> = self
            .conn
            .lock()
            .expect("library mutex")
            .query_row(
                "SELECT i.title,i.artist FROM import_items i JOIN tracks t ON t.id=i.track_id
             WHERE t.path=?1 AND i.status IN ('local','acquired')
             AND (?2=0 OR ABS(i.duration_ms-?2)<=5000)
             ORDER BY i.playlist_id,i.position LIMIT 1",
                params![path.to_string_lossy(), meta.duration_ms],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((title, artist)) = known {
            if meta.title == fallback {
                meta.title = title;
            }
            if meta.artist == "Unknown" {
                meta.artist = artist;
            }
        }
        Ok(())
    }

    /// Recover affected rows from linked import evidence, including older libraries.
    pub(super) fn restore_import_metadata(&self) -> Result<(), LibraryError> {
        for track in self.list_tracks()? {
            let fallback = Path::new(&track.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if track.title != fallback && track.artist != "Unknown" {
                continue;
            }
            let mut meta = FileMeta {
                title: track.title.clone(),
                artist: track.artist.clone(),
                album: track.album.clone(),
                duration_ms: track.duration_ms,
                isrc: track.isrc.clone(),
                artwork: None,
            };
            self.protect_import_metadata(Path::new(&track.path), &mut meta)?;
            if meta.title != track.title || meta.artist != track.artist {
                self.conn.lock().expect("library mutex").execute(
                    "UPDATE tracks SET title=?2,artist=?3 WHERE id=?1",
                    params![track.id.0, meta.title, meta.artist],
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reimport_and_reopen_restore_recording_identity_after_external_tags_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote-id.wav");
        fs::write(&path, b"audio").unwrap();
        let db = dir.path().join("library.db");
        let lib = Library::open(&db).unwrap();
        let id = lib.import_file(&path).unwrap();
        let playlist = lib.replace_playlist("Set", &[id]).unwrap();
        lib.save_import_items(
            playlist,
            &[ImportItem {
                album: None,
                position: 0,
                external_id: "remote-id".into(),
                title: "Song".into(),
                artist: "Artist".into(),
                duration_ms: 1000,
                status: "acquired".into(),
                track_id: Some(id),
                error: None,
            }],
        )
        .unwrap();
        lib.set_recording_metadata(id, "Song", "Artist", None)
            .unwrap();
        fs::write(&path, b"audio with external tag edit").unwrap();
        assert_eq!(lib.import_file(&path).unwrap(), id);
        let track = lib.get_track(id).unwrap();
        assert_eq!((&*track.title, &*track.artist), ("Song", "Artist"));
        lib.set_recording_metadata(id, "remote-id", "Unknown", None)
            .unwrap();
        drop(lib);
        let lib = Library::open(&db).unwrap();
        let track = lib.get_track(id).unwrap();
        assert_eq!((&*track.title, &*track.artist), ("Song", "Artist"));
        assert_eq!(lib.playlist_tracks(playlist).unwrap()[0].id, id);
    }
}
