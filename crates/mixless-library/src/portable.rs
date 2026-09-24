//! Versioned library records for portable packages. File extraction happens
//! before `restore`; all database changes then commit in one transaction.
use super::*;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortableTrack {
    pub track: Track,
    pub cues: Vec<Cue>,
    pub cue_version: Option<u32>,
    pub analysis: Option<(u32, TrackAnalysis)>,
    pub waveform: Option<PortableWaveform>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortableWaveform {
    pub version: u32,
    pub duration: f32,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortablePlaylist {
    pub id: i64,
    pub name: String,
    pub tracks: Vec<i64>,
    pub imports: Vec<ImportItem>,
}

pub struct Catalog {
    pub source: String,
    pub tracks: Vec<Track>,
    pub playlists: Vec<PortablePlaylist>,
}

impl Library {
    /// Capture membership and ordering under one database lock. Folder playlists
    /// become ordinary portable playlists: machine-specific watches do not travel.
    pub fn portable_catalog(
        &self,
        selection: Option<&[PlaylistId]>,
    ) -> Result<Catalog, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let source = conn.query_row(
            "SELECT identity FROM library_identity WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        let mut playlists = Vec::new();
        let mut query = conn.prepare("SELECT id,name FROM playlists ORDER BY id")?;
        for row in query.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, name) = row?;
            if selection.is_some_and(|s| !s.contains(&PlaylistId(id))) {
                continue;
            }
            let tracks = conn
                .prepare(
                    "SELECT track_id FROM playlist_items WHERE playlist_id=?1 ORDER BY position",
                )?
                .query_map([id], |r| r.get(0))?
                .collect::<Result<Vec<i64>, _>>()?;
            playlists.push(PortablePlaylist {
                id,
                name,
                tracks,
                imports: super::imports::read_import_items(&conn, PlaylistId(id))?,
            });
        }
        if selection.is_some_and(|ids| ids.iter().any(|id| !playlists.iter().any(|p| p.id == id.0)))
        {
            return Err(LibraryError::NotFound);
        }
        let selected: HashSet<_> = playlists
            .iter()
            .flat_map(|p| {
                p.tracks
                    .iter()
                    .copied()
                    .chain(p.imports.iter().filter_map(|i| i.track_id.map(|id| id.0)))
            })
            .collect();
        let tracks = conn.prepare("SELECT t.id,t.path,t.artwork_path,t.title,t.artist,t.album,t.duration_ms,t.isrc,t.bpm,t.key,t.camelot,t.analyzed,t.content_hash
            FROM tracks t LEFT JOIN library_order o ON o.track_id=t.id ORDER BY o.position IS NULL,o.position,t.artist,t.title,t.id")?
            .query_map([], row_to_track)?.collect::<Result<Vec<_>, _>>()?.into_iter()
            .filter(|t| selection.is_none() || selected.contains(&t.id.0)).collect();
        Ok(Catalog {
            source,
            tracks,
            playlists,
        })
    }

    /// A single track's derived records are captured together, with a revision
    /// guard against edits after the membership snapshot.
    pub fn portable_track(&self, expected: &Track) -> Result<PortableTrack, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let track = conn.query_row("SELECT id,path,artwork_path,title,artist,album,duration_ms,isrc,bpm,key,camelot,analyzed,content_hash FROM tracks WHERE id=?1",
            [expected.id.0], row_to_track)?;
        if track.content_hash != expected.content_hash || track.path != expected.path {
            return Err(LibraryError::SourceChanged);
        }
        let analysis: Option<(u32, String)> = conn
            .query_row(
                "SELECT version,payload FROM track_analysis WHERE track_id=?1 AND content_hash=?2",
                params![track.id.0, track.content_hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let analysis = analysis
            .map(|(version, payload)| serde_json::from_str(&payload).map(|a| (version, a)))
            .transpose()?;
        let waveform = conn.query_row("SELECT version,duration,payload FROM track_waveforms WHERE track_id=?1 AND content_hash=?2",
            params![track.id.0,track.content_hash], |r| Ok(PortableWaveform { version:r.get(0)?,duration:r.get(1)?,payload:r.get(2)? })).optional()?;
        let cue_version = conn
            .query_row(
                "SELECT version FROM cue_versions WHERE track_id=?1",
                [track.id.0],
                |r| r.get(0),
            )
            .optional()?;
        let cues = conn
            .prepare("SELECT idx,frame,kind,user_set FROM cues WHERE track_id=?1 ORDER BY idx")?
            .query_map([track.id.0], |r| {
                Ok(Cue {
                    index: r.get(0)?,
                    frame: r.get::<_, i64>(1)? as u64,
                    kind: match r.get::<_, String>(2)?.as_str() {
                        "in" => CueKind::In,
                        "out" => CueKind::Out,
                        _ => CueKind::Hot,
                    },
                    user_set: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PortableTrack {
            track,
            cues,
            cue_version,
            analysis,
            waveform,
        })
    }

    /// The loader reads one already verified, staged record at a time. Imported
    /// IDs are remapped, including analysis and remote placeholders. Re-imports
    /// update only records owned by the same source, without deleting local music.
    pub fn restore_portable_sources(
        &self,
        sources: &[(&str, &[i64], Vec<PortablePlaylist>)],
        mut load: impl FnMut(&str, i64) -> Result<PortableTrack, LibraryError>,
    ) -> Result<Vec<TrackId>, LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let mut restored = Vec::new();
        for &(source, order, ref playlists) in sources {
            let mut ids = BTreeMap::new();
            for &source_id in order {
                let mut record = load(source, source_id)?;
                let t = &record.track;
                let existing: Option<i64> = tx
                    .query_row(
                        "SELECT track_id FROM package_tracks WHERE source=?1 AND source_id=?2",
                        params![source, source_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                let id = if let Some(id) = existing {
                    id
                } else {
                    tx.execute("INSERT INTO tracks(path,title,artist,duration_ms,content_hash,mtime) VALUES(?1,?2,?3,?4,?5,0)", params![t.path,t.title,t.artist,t.duration_ms,t.content_hash])?;
                    let id = tx.last_insert_rowid();
                    tx.execute(
                        "INSERT INTO package_tracks(source,source_id,track_id) VALUES(?1,?2,?3)",
                        params![source, source_id, id],
                    )?;
                    id
                };
                tx.execute("UPDATE tracks SET path=?2,artwork_path=?3,title=?4,artist=?5,album=?6,duration_ms=?7,isrc=?8,bpm=?9,key=?10,camelot=?11,analyzed=?12,content_hash=?13,mtime=0 WHERE id=?1",
                params![id,t.path,t.artwork_path.as_deref().unwrap_or(""),t.title,t.artist,t.album,t.duration_ms,t.isrc,t.bpm,t.key,t.camelot,record.analysis.is_some(),t.content_hash])?;
                for table in ["track_analysis", "track_waveforms", "cues", "cue_versions"] {
                    tx.execute(&format!("DELETE FROM {table} WHERE track_id=?1"), [id])?;
                }
                if let Some((version, analysis)) = &mut record.analysis {
                    analysis.track_id = TrackId(id);
                    analysis.waveform_path = None;
                    tx.execute("INSERT INTO track_analysis(track_id,content_hash,version,payload) VALUES(?1,?2,?3,?4)", params![id,t.content_hash,*version,serde_json::to_string(analysis)?])?;
                }
                if let Some(w) = record.waveform {
                    tx.execute("INSERT INTO track_waveforms(track_id,content_hash,version,duration,payload) VALUES(?1,?2,?3,?4,?5)", params![id,t.content_hash,w.version,w.duration,w.payload])?;
                }
                for c in record.cues {
                    if c.index >= 8 || c.frame > i64::MAX as u64 {
                        return Err(LibraryError::CueIndex(c.index));
                    }
                    let kind = match c.kind {
                        CueKind::Hot => "hot",
                        CueKind::In => "in",
                        CueKind::Out => "out",
                    };
                    tx.execute(
                        "INSERT INTO cues(track_id,idx,frame,kind,user_set) VALUES(?1,?2,?3,?4,?5)",
                        params![id, c.index, c.frame as i64, kind, c.user_set],
                    )?;
                }
                if let Some(v) = record.cue_version {
                    tx.execute(
                        "INSERT INTO cue_versions(track_id,version) VALUES(?1,?2)",
                        params![id, v],
                    )?;
                }
                ids.insert(source_id, TrackId(id));
            }
            let map_id = |id: i64| ids.get(&id).copied().ok_or(LibraryError::NotFound);
            let source_key = format!("package:{source}");
            for p in playlists {
                let external_id = p.id.to_string();
                let existing: Option<i64> = tx.query_row("SELECT playlist_id FROM external_playlists WHERE source=?1 AND external_id=?2", params![source_key,external_id], |r| r.get(0)).optional()?;
                let id = if let Some(id) = existing {
                    id
                } else {
                    tx.execute("INSERT INTO playlists(name) VALUES(?1)", [&p.name])?;
                    let id = tx.last_insert_rowid();
                    tx.execute("INSERT INTO external_playlists(source,external_id,playlist_id) VALUES(?1,?2,?3)", params![source_key,external_id,id])?;
                    id
                };
                tx.execute(
                    "UPDATE playlists SET name=?2 WHERE id=?1",
                    params![id, p.name],
                )?;
                for table in ["playlist_items", "import_items", "playlist_exclusions"] {
                    tx.execute(&format!("DELETE FROM {table} WHERE playlist_id=?1"), [id])?;
                }
                for (pos, &track) in p.tracks.iter().enumerate() {
                    tx.execute("INSERT INTO playlist_items(playlist_id,position,track_id) VALUES(?1,?2,?3)", params![id,pos as i64,map_id(track)?.0])?;
                }
                for item in &p.imports {
                    let track = item
                        .track_id
                        .map(|t| map_id(t.0))
                        .transpose()?
                        .map(|id| id.0);
                    let status = if item.is_pending() {
                        "missing"
                    } else {
                        &item.status
                    };
                    tx.execute("INSERT INTO import_items(playlist_id,position,external_id,title,artist,duration_ms,status,track_id,error) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![id,item.position as i64,item.external_id,item.title,item.artist,item.duration_ms,status,track,item.error])?;
                }
            }
            // Preserve local rows; give imported rows the source library's order.
            for id in ids.values() {
                tx.execute("DELETE FROM library_order WHERE track_id=?1", [id.0])?;
            }
            let start: i64 = tx.query_row(
                "SELECT COALESCE(MAX(position)+1,0) FROM library_order",
                [],
                |r| r.get(0),
            )?;
            for (pos, source_id) in order.iter().enumerate() {
                tx.execute(
                    "INSERT INTO library_order(track_id,position) VALUES(?1,?2)",
                    params![map_id(*source_id)?.0, start + pos as i64],
                )?;
            }
            restored.extend(order.iter().map(|id| ids[id]));
        }
        tx.commit()?;
        Ok(restored)
    }

    pub fn package_media_dir(&self) -> PathBuf {
        self.db_path.with_file_name("package-media")
    }
}
