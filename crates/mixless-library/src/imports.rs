use super::*;

/// Every remote row is retained, including unavailable recordings and repeated songs.
#[derive(Debug, Clone, Serialize)]
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
        tx.commit()?;
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
