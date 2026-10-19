//! Missing album metadata is filled without changing audio, analysis or cues.
use super::*;

#[derive(Clone, Debug)]
pub struct SpotifyAlbumTarget {
    pub track_id: TrackId,
    pub content_hash: String,
    pub spotify_id: String,
    pub album: Option<String>,
}

impl Library {
    pub fn missing_spotify_albums(&self) -> Result<Vec<SpotifyAlbumTarget>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let mut stmt = conn.prepare(
            "SELECT DISTINCT t.id,t.content_hash,i.external_id,i.album
            FROM tracks t JOIN import_items i ON i.track_id=t.id
            JOIN external_playlists e ON e.playlist_id=i.playlist_id
            WHERE e.source='spotify' AND i.status IN ('local','acquired')
            AND NULLIF(TRIM(t.album),'') IS NULL ORDER BY t.id,i.album IS NULL,i.external_id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SpotifyAlbumTarget {
                    track_id: TrackId(r.get(0)?),
                    content_hash: r.get(1)?,
                    spotify_id: r.get(2)?,
                    album: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = std::collections::HashSet::new();
        Ok(rows
            .into_iter()
            .filter(|target| {
                target.spotify_id.len() == 22
                    && target.spotify_id.bytes().all(|c| c.is_ascii_alphanumeric())
                    && !target.content_hash.is_empty()
                    && seen.insert(target.track_id)
            })
            .collect())
    }

    /// Compare source revision and remote identity at commit time. Late responses
    /// cannot overwrite a tag, a replacement recording, or a removed import link.
    pub fn cache_spotify_album(
        &self,
        target: &SpotifyAlbumTarget,
        album: &str,
    ) -> Result<bool, LibraryError> {
        let album = album.trim();
        if album.is_empty() {
            return Ok(false);
        }
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let changed = tx.execute("UPDATE tracks SET album=?1 WHERE id=?2 AND content_hash=?3
            AND NULLIF(TRIM(album),'') IS NULL AND EXISTS (
                SELECT 1 FROM import_items i JOIN external_playlists e ON e.playlist_id=i.playlist_id
                WHERE i.track_id=?2 AND i.external_id=?4 AND i.status IN ('local','acquired') AND e.source='spotify')",
            params![album,target.track_id.0,target.content_hash,target.spotify_id])?;
        if changed > 0 {
            tx.execute("UPDATE import_items SET album=?1 WHERE track_id=?2 AND external_id=?3
                AND NULLIF(TRIM(album),'') IS NULL AND playlist_id IN (SELECT playlist_id FROM external_playlists WHERE source='spotify')",
                params![album,target.track_id.0,target.spotify_id])?;
        }
        tx.commit()?;
        drop(conn);
        if changed > 0 {
            self.write_track_metadata_if_possible(target.track_id);
        }
        Ok(changed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Library, PathBuf, PlaylistId, TrackId) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let audio = dir.path().join("song.mp3");
        fs::write(&audio, b"fixture audio").unwrap();
        let id = lib.import_file(&audio).unwrap();
        let playlist = lib
            .external_playlist("spotify", "playlist", "Playlist")
            .unwrap();
        lib.save_import_items(
            playlist,
            &[ImportItem {
                position: 0,
                external_id: "a".repeat(22),
                title: "Song".into(),
                artist: "Artist".into(),
                album: None,
                duration_ms: 1000,
                status: "local".into(),
                track_id: Some(id),
                error: None,
            }],
        )
        .unwrap();
        (dir, lib, audio, playlist, id)
    }

    #[test]
    fn fills_old_imports_preserves_tags_and_survives_reimport_and_restart() {
        let (dir, lib, audio, _, id) = fixture();
        let before = lib.get_track(id).unwrap();
        let target = lib.missing_spotify_albums().unwrap().remove(0);
        assert!(lib.cache_spotify_album(&target, " Album ").unwrap());
        assert!(!lib.cache_spotify_album(&target, "Other").unwrap());
        assert!(lib.missing_spotify_albums().unwrap().is_empty());
        lib.import_file(&audio).unwrap();
        let after = lib.get_track(id).unwrap();
        assert_eq!(after.album.as_deref(), Some("Album"));
        assert_eq!(before.content_hash, after.content_hash);
        assert_eq!(before.analyzed, after.analyzed);
        assert_eq!(fs::read(&audio).unwrap(), b"fixture audio");
        drop(lib);
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        assert_eq!(lib.get_track(id).unwrap().album.as_deref(), Some("Album"));
    }

    #[test]
    fn stale_audio_or_link_cannot_receive_an_album() {
        let (_dir, lib, audio, playlist, id) = fixture();
        let target = lib.missing_spotify_albums().unwrap().remove(0);
        lib.save_import_items(playlist, &[]).unwrap();
        assert!(!lib.cache_spotify_album(&target, "Wrong").unwrap());
        fs::write(&audio, b"replacement").unwrap();
        lib.import_file(&audio).unwrap();
        assert!(!lib.cache_spotify_album(&target, "Wrong").unwrap());
        assert_eq!(lib.get_track(id).unwrap().album, None);
    }

    #[test]
    fn ready_link_and_retry_apply_album_from_the_import_snapshot() {
        let (_dir, lib, _, playlist, id) = fixture();
        let mut row = lib.import_items(playlist).unwrap().remove(0);
        row.album = Some("Remote album".into());
        row.status = "missing".into();
        row.track_id = None;
        lib.save_import_items(playlist, &[row.clone()]).unwrap();
        assert_eq!(lib.get_track(id).unwrap().album, None);
        let (_, mut retry) = lib
            .begin_import_retry(playlist, 0, &row.external_id)
            .unwrap()
            .unwrap();
        assert_eq!(retry.album, row.album);
        retry.status = "acquired".into();
        retry.track_id = Some(id);
        lib.update_import_items(playlist, &[retry]).unwrap();
        assert_eq!(lib.get_track(id).unwrap().album, row.album);
        row.status = "local".into();
        row.track_id = Some(id);
        row.album = Some("Different".into());
        lib.sync_import_items(playlist, "playlist", "Synced", &[row])
            .unwrap();
        assert_eq!(
            lib.get_track(id).unwrap().album.as_deref(),
            Some("Remote album")
        );
    }
}
