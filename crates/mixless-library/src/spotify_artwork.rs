use super::*;

#[derive(Clone, Debug)]
pub struct SpotifyArtworkTarget {
    pub track_id: TrackId,
    pub content_hash: String,
    pub spotify_id: String,
}

impl Library {
    /// Includes old imports whose files were checked and had no embedded art.
    /// Only an existing Spotify recording link can supply a cover identity.
    pub fn missing_spotify_artwork(&self) -> Result<Vec<SpotifyArtworkTarget>, LibraryError> {
        let rows = {
            let conn = self.conn.lock().expect("library mutex");
            let mut stmt = conn.prepare(
                "SELECT DISTINCT t.id,t.content_hash,i.external_id,t.artwork_path
                 FROM tracks t JOIN import_items i ON i.track_id=t.id
                 JOIN external_playlists e ON e.playlist_id=i.playlist_id
                 WHERE e.source='spotify' AND i.status IN ('local','acquired')
                 ORDER BY t.id,i.external_id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    SpotifyArtworkTarget {
                        track_id: TrackId(row.get(0)?),
                        content_hash: row.get(1)?,
                        spotify_id: row.get(2)?,
                    },
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut seen = std::collections::HashSet::new();
        Ok(rows
            .into_iter()
            .filter_map(|(target, path)| {
                (target.spotify_id.len() == 22
                    && target.spotify_id.bytes().all(|c| c.is_ascii_alphanumeric())
                    && !target.content_hash.is_empty()
                    && !path
                        .as_deref()
                        .is_some_and(|path| Path::new(path).is_file())
                    && seen.insert(target.track_id))
                .then_some(target)
            })
            .collect())
    }

    /// Called with decoded/validated image bytes from a worker. Never replace
    /// an existing cover or attach a late response to changed audio.
    pub fn cache_spotify_artwork(
        &self,
        target: &SpotifyArtworkTarget,
        bytes: &[u8],
    ) -> Result<bool, LibraryError> {
        let extension = sniff_image_extension(bytes).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "unsupported cover image")
        })?;
        let conn = self.conn.lock().expect("library mutex");
        let current: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT content_hash,artwork_path FROM tracks WHERE id=?1",
                [target.track_id.0],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((hash, path)) = current else {
            return Ok(false);
        };
        if hash != target.content_hash
            || path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_file())
        {
            return Ok(false);
        }
        let path = self.cache_artwork(
            &hash,
            Some(&Artwork {
                data: bytes.to_vec(),
                extension,
            }),
        )?;
        conn.execute(
            "UPDATE tracks SET artwork_path=?1 WHERE id=?2",
            params![path, target.track_id.0],
        )?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nvalidated image fixture";

    fn fixture() -> (tempfile::TempDir, Library, TrackId, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(&dir.path().join("library.db")).unwrap();
        let path = dir.path().join("track.wav");
        fs::write(&path, b"audio fixture").unwrap();
        let id = library.import_file(&path).unwrap();
        let playlist = library
            .external_playlist("spotify", "playlist", "List")
            .unwrap();
        library
            .save_import_items(
                playlist,
                &[0, 1].map(|position| ImportItem {
                    position,
                    external_id: "a".repeat(22),
                    title: "Track".into(),
                    artist: "Artist".into(),
                    duration_ms: 1000,
                    status: "acquired".into(),
                    track_id: Some(id),
                    error: None,
                }),
            )
            .unwrap();
        (dir, library, id, path)
    }

    #[test]
    fn downloaded_covers_backfill_once_and_survive_reimport_and_reopen() {
        let (dir, library, id, path) = fixture();
        let targets = library.missing_spotify_artwork().unwrap();
        assert_eq!(targets.len(), 1, "duplicate playlist entries share artwork");
        let hash = library.get_track(id).unwrap().content_hash;
        assert!(library.cache_spotify_artwork(&targets[0], PNG).unwrap());
        let cover = library.get_track(id).unwrap().artwork_path.unwrap();
        assert_eq!(fs::read(&cover).unwrap(), PNG);
        assert!(library.missing_spotify_artwork().unwrap().is_empty());
        assert!(!library
            .cache_spotify_artwork(&targets[0], b"\xff\xd8\xffother cover")
            .unwrap());
        library.import_file(&path).unwrap();
        library.backfill_artwork().unwrap();
        drop(library);
        let library = Library::open(&dir.path().join("library.db")).unwrap();
        let track = library.get_track(id).unwrap();
        assert_eq!(track.artwork_path.as_deref(), Some(cover.as_str()));
        assert_eq!(
            track.content_hash, hash,
            "fetching artwork must not rewrite audio"
        );
        library.clear_artwork(None).unwrap();
        assert_eq!(library.missing_spotify_artwork().unwrap().len(), 1);
        assert!(library.cache_spotify_artwork(&targets[0], PNG).unwrap());
    }

    #[test]
    fn late_artwork_cannot_attach_to_changed_or_removed_recording() {
        let (_dir, library, id, path) = fixture();
        let target = library.missing_spotify_artwork().unwrap().remove(0);
        fs::write(&path, b"different and longer audio").unwrap();
        library.import_file(&path).unwrap();
        assert!(!library.cache_spotify_artwork(&target, PNG).unwrap());
        assert!(library.get_track(id).unwrap().artwork_path.is_none());
        library
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM tracks WHERE id=?1", [id.0])
            .unwrap();
        assert!(!library.cache_spotify_artwork(&target, PNG).unwrap());
    }
}
