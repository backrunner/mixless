use super::*;

/// Every remote row is retained, including unavailable recordings and repeated songs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportItem {
    pub position: usize,
    pub external_id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u32,
    pub status: String,
    pub track_id: Option<TrackId>,
    pub error: Option<String>,
}

impl ImportItem {
    pub fn is_ready(&self) -> bool {
        matches!(self.status.as_str(), "local" | "acquired") && self.track_id.is_some()
    }

    pub fn is_pending(&self) -> bool {
        matches!(self.status.as_str(), "queued" | "resolving" | "analyzing")
    }

    pub fn is_failed(&self) -> bool {
        matches!(self.status.as_str(), "missing" | "suspect")
    }
}

impl Library {
    /// Remote identity, never the display name, decides which playlist to update.
    pub fn external_playlist(
        &self,
        source: &str,
        external_id: &str,
        name: &str,
    ) -> Result<PlaylistId, LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT playlist_id FROM external_playlists WHERE source=?1 AND external_id=?2",
                params![source, external_id],
                |r| r.get(0),
            )
            .optional()?;
        let id = if let Some(id) = existing {
            tx.execute(
                "UPDATE playlists SET name=?1 WHERE id=?2",
                params![name, id],
            )?;
            id
        } else {
            tx.execute("INSERT INTO playlists(name) VALUES (?1)", [name])?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO external_playlists(source,external_id,playlist_id) VALUES (?1,?2,?3)",
                params![source, external_id, id],
            )?;
            id
        };
        tx.commit()?;
        Ok(PlaylistId(id))
    }

    pub fn import_items(&self, playlist: PlaylistId) -> Result<Vec<ImportItem>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        read_import_items(&conn, playlist)
    }

    /// Claim one failed Spotify row without replacing playlist membership or
    /// ordering. A second click, stale row or deleted playlist cannot restart it.
    pub fn begin_import_retry(
        &self,
        playlist: PlaylistId,
        position: usize,
        external_id: &str,
    ) -> Result<Option<(String, ImportItem)>, LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let name: Option<String> = tx
            .query_row(
                "SELECT p.name FROM playlists p JOIN external_playlists e ON e.playlist_id=p.id
             WHERE p.id=?1 AND e.source='spotify'",
                [playlist.0],
                |r| r.get(0),
            )
            .optional()?;
        let Some(name) = name else { return Ok(None) };
        let Some(mut item) = read_import_items(&tx, playlist)?.into_iter().find(|item| {
            item.position == position && item.external_id == external_id && item.is_failed()
        }) else {
            return Ok(None);
        };
        tx.execute(
            "UPDATE import_items SET status='queued',track_id=NULL,error=NULL
             WHERE playlist_id=?1 AND position=?2 AND external_id=?3
             AND status IN ('missing','suspect')",
            params![playlist.0, position as i64, external_id],
        )?;
        tx.commit()?;
        item.status = "queued".into();
        item.track_id = None;
        item.error = None;
        Ok(Some((name, item)))
    }

    /// One snapshot keeps placeholder slots and playable rows consistent while
    /// workers finish tracks between UI refreshes.
    pub fn playlist_content(
        &self,
        playlist: PlaylistId,
    ) -> Result<(Vec<Track>, Vec<ImportItem>), LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let tx = conn.unchecked_transaction()?;
        let tracks = {
            let mut query = tx.prepare(
                "SELECT t.id,t.path,t.artwork_path,t.title,t.artist,t.album,t.duration_ms,t.isrc,
                        t.bpm,t.key,t.camelot,t.analyzed,t.content_hash
                 FROM playlist_items i JOIN tracks t ON t.id=i.track_id
                 WHERE i.playlist_id=?1 ORDER BY i.position",
            )?;
            let rows = query.query_map([playlist.0], row_to_track)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let imports = read_import_items(&tx, playlist)?;
        tx.commit()?;
        Ok((tracks, imports))
    }

    /// Atomic replacement, so failures never leave half a playlist. Only ready
    /// local/acquired rows enter the playable queue; suspect/missing remain visible.
    pub fn save_import_items(
        &self,
        playlist: PlaylistId,
        items: &[ImportItem],
    ) -> Result<(), LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let ready: Vec<_> = items
            .iter()
            .filter(|i| matches!(i.status.as_str(), "local" | "acquired"))
            .filter_map(|i| i.track_id)
            .collect();
        let ordered = super::ordering::refreshed_order(&tx, playlist, &ready)?;
        tx.execute(
            "DELETE FROM playlist_items WHERE playlist_id=?1",
            [playlist.0],
        )?;
        tx.execute(
            "DELETE FROM import_items WHERE playlist_id=?1",
            [playlist.0],
        )?;
        for item in items {
            tx.execute("INSERT INTO import_items(playlist_id,position,external_id,title,artist,duration_ms,status,track_id,error) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![playlist.0,item.position as i64,item.external_id,item.title,item.artist,item.duration_ms,item.status,item.track_id.map(|id|id.0),item.error])?;
        }
        for (position, id) in ordered.iter().enumerate() {
            tx.execute(
                "INSERT INTO playlist_items(playlist_id,position,track_id) VALUES (?1,?2,?3)",
                params![playlist.0, position as i64, id.0],
            )?;
        }
        align_import_order(&tx, playlist, &ordered)?;
        tx.commit()?;
        Ok(())
    }

    /// Update existing rows only: a user removal during acquisition is final.
    /// Rebuild playable membership in row order, never in worker completion order.
    pub fn update_import_items(
        &self,
        playlist: PlaylistId,
        items: &[ImportItem],
    ) -> Result<(), LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        for item in items {
            tx.execute(
                "UPDATE import_items SET status=?1,track_id=?2,error=?3
                 WHERE playlist_id=?4 AND position=?5 AND external_id=?6
                 AND status IN ('queued','resolving','analyzing')",
                params![
                    item.status,
                    item.track_id.map(|id| id.0),
                    item.error,
                    playlist.0,
                    item.position as i64,
                    item.external_id
                ],
            )?;
        }
        if items.iter().any(|item| !item.is_pending()) {
            tx.execute(
                "DELETE FROM playlist_items WHERE playlist_id=?1",
                [playlist.0],
            )?;
            tx.execute(
                "INSERT INTO playlist_items(playlist_id,position,track_id)
                 SELECT playlist_id,position,track_id FROM import_items
                 WHERE playlist_id=?1 AND status IN ('local','acquired') AND track_id IS NOT NULL",
                [playlist.0],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_import_item(
        &self,
        playlist: PlaylistId,
        position: usize,
    ) -> Result<(), LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        conn.execute(
            "DELETE FROM import_items WHERE playlist_id=?1 AND position=?2
             AND (status NOT IN ('local','acquired') OR track_id IS NULL)",
            params![playlist.0, position as i64],
        )?;
        Ok(())
    }

    pub fn linked_import_track(&self, external_id: &str) -> Result<Option<TrackId>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        Ok(conn.query_row("SELECT track_id FROM import_items WHERE external_id=?1 AND status IN ('local','acquired') AND track_id IS NOT NULL LIMIT 1", [external_id], |r| r.get::<_,i64>(0)).optional()?.map(TrackId))
    }

    pub fn set_recording_metadata(
        &self,
        id: TrackId,
        title: &str,
        artist: &str,
        isrc: Option<&str>,
    ) -> Result<(), LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        conn.execute(
            "UPDATE tracks SET title=?1,artist=?2,isrc=COALESCE(?3,isrc) WHERE id=?4",
            params![title, artist, isrc, id.0],
        )?;
        Ok(())
    }

    /// Finish decode/analysis and its displayed metadata in one revision check.
    pub fn finish_analysis(
        &self,
        analysis: &TrackAnalysis,
        hash: &str,
        version: u32,
    ) -> Result<(), LibraryError> {
        let payload = serde_json::to_string(analysis)?;
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let changed = tx.execute("UPDATE tracks SET duration_ms=?1,bpm=?2,key=?3,camelot=?4,analyzed=1 WHERE id=?5 AND content_hash=?6",
            params![(analysis.duration_sec * 1000.).round() as u32,analysis.tempo.global_bpm,analysis.key,analysis.camelot,analysis.track_id.0,hash])?;
        if changed != 1 {
            return Err(LibraryError::NotFound);
        }
        tx.execute("INSERT INTO track_analysis(track_id,content_hash,version,payload) VALUES (?1,?2,?3,?4) ON CONFLICT(track_id) DO UPDATE SET content_hash=excluded.content_hash,version=excluded.version,payload=excluded.payload",
            params![analysis.track_id.0,hash,version,payload])?;
        tx.commit()?;
        Ok(())
    }
}

fn read_import_items(
    conn: &Connection,
    playlist: PlaylistId,
) -> Result<Vec<ImportItem>, LibraryError> {
    let mut query = conn.prepare("SELECT position,external_id,title,artist,duration_ms,status,track_id,error FROM import_items WHERE playlist_id=?1 ORDER BY position")?;
    let rows = query.query_map([playlist.0], |r| {
        Ok(ImportItem {
            position: r.get::<_, i64>(0)? as usize,
            external_id: r.get(1)?,
            title: r.get(2)?,
            artist: r.get(3)?,
            duration_ms: r.get(4)?,
            status: r.get(5)?,
            track_id: r.get::<_, Option<i64>>(6)?.map(TrackId),
            error: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Manual moves permute completed rows in their existing slots. Pending rows
/// keep their identity/position so in-flight completions still address them.
pub(super) fn align_import_order(
    tx: &rusqlite::Transaction<'_>,
    playlist: PlaylistId,
    ordered: &[TrackId],
) -> Result<(), LibraryError> {
    let mut query = tx.prepare(
        "SELECT position,external_id,title,artist,duration_ms,status,track_id,error
         FROM import_items WHERE playlist_id=?1 AND status IN ('local','acquired')
         AND track_id IS NOT NULL ORDER BY position",
    )?;
    let rows = query.query_map([playlist.0], |r| {
        Ok(ImportItem {
            position: r.get::<_, i64>(0)? as usize,
            external_id: r.get(1)?,
            title: r.get(2)?,
            artist: r.get(3)?,
            duration_ms: r.get(4)?,
            status: r.get(5)?,
            track_id: r.get::<_, Option<i64>>(6)?.map(TrackId),
            error: r.get(7)?,
        })
    })?;
    let items = rows.collect::<Result<Vec<_>, _>>()?;
    let mut by_track = std::collections::HashMap::<_, std::collections::VecDeque<_>>::new();
    for item in &items {
        by_track
            .entry(item.track_id.unwrap())
            .or_default()
            .push_back(item);
    }
    for (slot, id) in items.iter().zip(ordered) {
        let Some(item) = by_track.get_mut(id).and_then(|items| items.pop_front()) else {
            continue;
        };
        tx.execute(
            "UPDATE import_items SET external_id=?1,title=?2,artist=?3,duration_ms=?4,
             status=?5,track_id=?6,error=?7 WHERE playlist_id=?8 AND position=?9",
            params![
                item.external_id,
                item.title,
                item.artist,
                item.duration_ms,
                item.status,
                item.track_id.map(|id| id.0),
                item.error,
                playlist.0,
                slot.position as i64
            ],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_summaries_count_failed_rows_independently_of_the_selected_playlist() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let first = lib.external_playlist("spotify", "first", "First").unwrap();
        let second = lib
            .external_playlist("spotify", "second", "Second")
            .unwrap();
        let manual = lib.replace_playlist("Manual", &[]).unwrap();
        let rows: Vec<_> = ["missing", "suspect", "queued", "analyzing"]
            .iter()
            .enumerate()
            .map(|(position, status)| ImportItem {
                position,
                external_id: format!("row-{position}"),
                title: "Song".into(),
                artist: "Artist".into(),
                duration_ms: 1000,
                status: (*status).into(),
                track_id: None,
                error: None,
            })
            .collect();
        lib.save_import_items(first, &rows).unwrap();
        lib.save_import_items(second, &rows[..1]).unwrap();
        let failed = |id: PlaylistId| {
            lib.list_playlists()
                .unwrap()
                .iter()
                .find(|p| p.id == id.0)
                .unwrap()
                .failed_imports
        };
        assert_eq!((failed(first), failed(second), failed(manual)), (2, 1, 0));
        lib.begin_import_retry(first, 0, "row-0").unwrap().unwrap();
        assert_eq!((failed(first), failed(second)), (1, 1));
        lib.remove_import_item(first, 1).unwrap();
        assert_eq!((failed(first), failed(second)), (0, 1));
    }

    #[test]
    fn incremental_rows_keep_order_exclude_failures_and_respect_removal_and_reordering() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("library.db");
        let lib = Library::open(&db).unwrap();
        let tracks: Vec<_> = (0..4)
            .map(|i| {
                let path = dir.path().join(format!("{i}.wav"));
                fs::write(&path, b"fixture").unwrap();
                lib.register_local_files(&[path]).unwrap()[0]
            })
            .collect();
        let playlist = lib
            .external_playlist("spotify", "remote", "Remote")
            .unwrap();
        let mut rows: Vec<_> = (0..5)
            .map(|position| ImportItem {
                position,
                external_id: format!("id-{position}"),
                title: format!("Song {position}"),
                artist: "Artist".into(),
                duration_ms: 1000,
                status: "queued".into(),
                track_id: None,
                error: None,
            })
            .collect();
        lib.save_import_items(playlist, &rows).unwrap();
        assert_eq!(
            lib.list_playlists()
                .unwrap()
                .iter()
                .find(|p| p.id == playlist.0)
                .unwrap()
                .tracks,
            5
        );
        assert!(lib.playlist_tracks(playlist).unwrap().is_empty());
        for (position, id) in [(3, tracks[1]), (0, tracks[0])] {
            rows[position].track_id = Some(id);
            rows[position].status = "acquired".into();
            lib.update_import_items(playlist, &[rows[position].clone()])
                .unwrap();
        }
        let ids = |lib: &Library| {
            lib.playlist_tracks(playlist)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&lib), tracks[..2]);
        lib.reorder_tracks(Some(playlist), &tracks[..2], &[tracks[1], tracks[0]])
            .unwrap();
        rows[1].status = "missing".into();
        rows[1].error = Some("Unavailable".into());
        lib.update_import_items(playlist, &[rows[1].clone()])
            .unwrap();
        lib.remove_import_item(playlist, 1).unwrap();
        // Late worker notifications and new successes cannot restore a removed row.
        rows[2].status = "acquired".into();
        rows[2].track_id = Some(tracks[2]);
        lib.update_import_items(playlist, &rows[1..3]).unwrap();
        assert_eq!(ids(&lib), [tracks[1], tracks[2], tracks[0]]);
        lib.remove_playlist_track(playlist, tracks[1]).unwrap();
        rows[4].status = "suspect".into();
        // Even an accidental ID on a failed row must never make it playable.
        rows[4].track_id = Some(tracks[3]);
        lib.update_import_items(playlist, &rows[4..]).unwrap();
        drop(lib);
        let lib = Library::open(&db).unwrap();
        let (ready, imports) = lib.playlist_content(playlist).unwrap();
        assert_eq!(
            ready.iter().map(|t| t.id).collect::<Vec<_>>(),
            [tracks[2], tracks[0]]
        );
        assert_eq!(
            imports.iter().map(|i| i.position).collect::<Vec<_>>(),
            [2, 3, 4]
        );
        assert_eq!(
            lib.list_playlists()
                .unwrap()
                .iter()
                .find(|p| p.id == playlist.0)
                .unwrap()
                .tracks,
            3
        );
    }
}
