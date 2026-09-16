//! Derived-data accounting and explicit cleanup. Source audio, saved cues,
//! playlists and import rows are never touched — only regenerable data.
use super::*;

/// Byte totals for regenerable per-track data held in the database.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheUsage {
    pub tracks: usize,
    pub analysis_bytes: u64,
    pub waveform_bytes: u64,
}

const MEMBERS: &str = "(SELECT track_id FROM playlist_items WHERE playlist_id=?1)";

fn payload_bytes(
    conn: &Connection,
    table: &str,
    playlist: Option<PlaylistId>,
) -> Result<u64, LibraryError> {
    let sql = match playlist {
        Some(_) => format!(
            "SELECT COALESCE(SUM(LENGTH(payload)),0) FROM {table} WHERE track_id IN {MEMBERS}"
        ),
        None => format!("SELECT COALESCE(SUM(LENGTH(payload)),0) FROM {table}"),
    };
    Ok(conn.query_row(
        &sql,
        rusqlite::params_from_iter(playlist.map(|p| p.0)),
        |r| r.get::<_, i64>(0),
    )? as u64)
}

/// Recursive total without following links; unreadable entries count as zero.
fn dir_bytes(path: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![path.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

impl Library {
    /// Payload bytes kept for the whole library or one playlist's members.
    pub fn cache_usage(&self, playlist: Option<PlaylistId>) -> Result<CacheUsage, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let tracks: i64 = match playlist {
            Some(id) => {
                conn.query_row(&format!("SELECT COUNT(*) FROM {MEMBERS}"), [id.0], |r| {
                    r.get(0)
                })?
            }
            None => conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))?,
        };
        Ok(CacheUsage {
            tracks: tracks as usize,
            analysis_bytes: payload_bytes(&conn, "track_analysis", playlist)?,
            waveform_bytes: payload_bytes(&conn, "track_waveforms", playlist)?,
        })
    }

    /// `(id, content hash)` pairs addressing each track's on-disk caches.
    pub fn cache_keys(
        &self,
        playlist: Option<PlaylistId>,
    ) -> Result<Vec<(TrackId, String)>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let sql = match playlist {
            Some(_) => format!("SELECT id, content_hash FROM tracks WHERE id IN {MEMBERS}"),
            None => "SELECT id, content_hash FROM tracks".into(),
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(playlist.map(|p| p.0)), |r| {
            Ok((TrackId(r.get::<_, i64>(0)?), r.get::<_, String>(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Drop analysis payloads and mark the tracks un-analysed so a later visit
    /// recomputes them. Saved cues stay; generated cue pads and the cue
    /// version marker go away with the analysis that produced them.
    /// Returns the freed payload bytes (the file itself compacts on checkpoint).
    pub fn clear_analysis(&self, playlist: Option<PlaylistId>) -> Result<u64, LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let freed = payload_bytes(&tx, "track_analysis", playlist)?;
        match playlist {
            Some(id) => {
                tx.execute(
                    &format!("UPDATE tracks SET analyzed=0,bpm=NULL,key=NULL,camelot=NULL WHERE id IN {MEMBERS}"),
                    [id.0],
                )?;
                tx.execute(
                    &format!("DELETE FROM track_analysis WHERE track_id IN {MEMBERS}"),
                    [id.0],
                )?;
                tx.execute(
                    &format!("DELETE FROM cues WHERE track_id IN {MEMBERS} AND user_set=0"),
                    [id.0],
                )?;
                tx.execute(
                    &format!("DELETE FROM cue_versions WHERE track_id IN {MEMBERS}"),
                    [id.0],
                )?;
            }
            None => {
                tx.execute(
                    "UPDATE tracks SET analyzed=0,bpm=NULL,key=NULL,camelot=NULL",
                    [],
                )?;
                tx.execute("DELETE FROM track_analysis", [])?;
                tx.execute("DELETE FROM cues WHERE user_set=0", [])?;
                tx.execute("DELETE FROM cue_versions", [])?;
            }
        }
        tx.commit()?;
        Ok(freed)
    }

    /// Drop stored spectral envelopes; playback recomputes them from audio.
    pub fn clear_waveforms(&self, playlist: Option<PlaylistId>) -> Result<u64, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let freed = payload_bytes(&conn, "track_waveforms", playlist)?;
        match playlist {
            Some(id) => conn.execute(
                &format!("DELETE FROM track_waveforms WHERE track_id IN {MEMBERS}"),
                [id.0],
            )?,
            None => conn.execute("DELETE FROM track_waveforms", [])?,
        };
        Ok(freed)
    }

    /// Extracted cover files live beside the database, named by content hash.
    pub fn artwork_dir(&self) -> &Path {
        &self.artwork_dir
    }

    /// Cover files whose content hash matches `hashes`, or the whole directory.
    pub fn artwork_bytes(&self, hashes: Option<&[String]>) -> u64 {
        match hashes {
            Some(hashes) => hashes
                .iter()
                .flat_map(|hash| self.artwork_files(hash))
                .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
                .sum(),
            None => dir_bytes(&self.artwork_dir),
        }
    }

    /// Delete extracted covers — all of them, or only those owned by `hashes`.
    /// Rows pointing at a missing file reset to NULL so the next artwork
    /// backfill re-extracts them from the source audio.
    pub fn clear_artwork(&self, hashes: Option<&[String]>) -> Result<u64, LibraryError> {
        let targets: Vec<PathBuf> = match hashes {
            Some(hashes) => hashes
                .iter()
                .flat_map(|hash| self.artwork_files(hash))
                .collect(),
            None => fs::read_dir(&self.artwork_dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .collect(),
        };
        let mut freed = 0;
        for path in targets {
            freed += fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            fs::remove_file(path)?;
        }
        // A cover can be shared with tracks outside the cleared scope, so the
        // reconciliation sweep checks every row, not just the cleared set.
        let missing: Vec<i64> = {
            let conn = self.conn.lock().expect("library mutex");
            let mut stmt = conn.prepare(
                "SELECT id, artwork_path FROM tracks
                 WHERE artwork_path IS NOT NULL AND artwork_path != ''",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            rows.filter_map(|r| r.ok())
                .filter(|(_, path)| !Path::new(path).exists())
                .map(|(id, _)| id)
                .collect()
        };
        if !missing.is_empty() {
            let mut conn = self.conn.lock().expect("library mutex");
            let tx = conn.transaction()?;
            for id in missing {
                tx.execute("UPDATE tracks SET artwork_path=NULL WHERE id=?1", [id])?;
            }
            tx.commit()?;
        }
        Ok(freed)
    }

    /// On-disk size of the database and its WAL companions.
    pub fn database_bytes(&self) -> u64 {
        [
            self.db_path.clone(),
            sidecar(&self.db_path, "-wal"),
            sidecar(&self.db_path, "-shm"),
        ]
        .iter()
        .map(|path| fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        .sum()
    }

    fn artwork_files(&self, hash: &str) -> Vec<PathBuf> {
        fs::read_dir(&self.artwork_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file() && path.file_stem().and_then(|s| s.to_str()) == Some(hash)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{CueKind, Waveform};

    fn fixture() -> (tempfile::TempDir, Library, Vec<TrackId>) {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Set");
        fs::create_dir(&folder).unwrap();
        let a = folder.join("a.wav");
        let b = folder.join("b.wav");
        fs::write(&a, b"first").unwrap();
        fs::write(&b, b"second").unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let ids = lib.register_local_files(&[a.clone(), b.clone()]).unwrap();
        for path in [a, b] {
            lib.import_file(&path).unwrap();
        }
        (dir, lib, ids)
    }

    fn analysis(id: TrackId) -> TrackAnalysis {
        TrackAnalysis {
            track_id: id,
            duration_sec: 10.,
            sample_rate: 44100,
            tempo: mixless_protocol::TempoMap::default(),
            key: None,
            camelot: None,
            key_confidence: 0.,
            sections: vec![],
            phrase_boundaries: vec![],
            mix_regions: vec![],
            bars: vec![],
            moments: vec![],
            stems: None,
            waveform_path: None,
            partial: false,
        }
    }

    fn wave() -> Waveform {
        Waveform {
            columns: 2,
            duration_sec: 4.,
            peak: vec![1, 2],
            peak_pos: vec![1, 2],
            peak_neg: vec![2, 1],
            rms: vec![1, 1],
            low: vec![255, 0],
            low_mid: vec![0; 2],
            mid: vec![0; 2],
            high: vec![0; 2],
            detail_pos: vec![257, 514],
            detail_neg: vec![514, 257],
            detail_rms: vec![257, 257],
        }
    }

    #[test]
    fn scoped_usage_and_clearing_keep_membership_audio_and_user_cues() {
        let (_dir, lib, ids) = fixture();
        let folder = PlaylistId(lib.list_playlists().unwrap()[0].id);
        let spare = lib.replace_playlist("Spare", &[ids[0]]).unwrap();
        for id in ids.iter().copied() {
            let hash = lib.get_track(id).unwrap().content_hash;
            lib.finish_analysis(&analysis(id), &hash, 1).unwrap();
            lib.save_waveform(id, &hash, &wave()).unwrap();
            lib.set_cue(id, 0, 48000, CueKind::Hot, true).unwrap();
            lib.set_cue(id, 1, 96000, CueKind::Hot, false).unwrap();
        }
        let all = lib.cache_usage(None).unwrap();
        let scoped = lib.cache_usage(Some(folder)).unwrap();
        assert_eq!((all.tracks, scoped.tracks), (2, 2));
        assert!(all.analysis_bytes > 0 && all.waveform_bytes > 0);
        assert_eq!(all, scoped);
        assert_eq!(lib.cache_usage(Some(spare)).unwrap().tracks, 1);

        lib.clear_waveforms(Some(folder)).unwrap();
        lib.clear_analysis(Some(folder)).unwrap();
        let cleared = lib.cache_usage(Some(folder)).unwrap();
        assert_eq!((cleared.analysis_bytes, cleared.waveform_bytes), (0, 0));
        // Member tracks lose derived state but keep files, rows and user cues.
        for id in ids {
            let track = lib.get_track(id).unwrap();
            assert!(!track.analyzed);
            assert_eq!(lib.cues(id).unwrap().len(), 1);
            assert!(lib.cues(id).unwrap()[0].user_set);
            assert!(Path::new(&track.path).exists());
        }
        assert_eq!(lib.playlist_tracks(folder).unwrap().len(), 2);
        assert!(lib.database_bytes() > 0);
    }

    #[test]
    fn artwork_clearing_recovers_orphans_and_resets_rows_for_backfill() {
        let (dir, lib, ids) = fixture();
        // other.wav duplicates a.wav's bytes, so both tracks share one cover.
        let other = dir.path().join("other.wav");
        fs::write(&other, b"first").unwrap();
        let shared = lib.import_file(&other).unwrap();
        for id in ids.iter().copied().chain([shared]) {
            let track = lib.get_track(id).unwrap();
            let path = lib
                .cache_artwork(
                    &track.content_hash,
                    Some(&Artwork {
                        data: vec![1, 2, 3],
                        extension: "png",
                    }),
                )
                .unwrap()
                .unwrap();
            lib.conn
                .lock()
                .unwrap()
                .execute(
                    "UPDATE tracks SET artwork_path=?1 WHERE id=?2",
                    params![path, id.0],
                )
                .unwrap();
        }
        assert_eq!(lib.artwork_bytes(None), 6);
        let folder_hash = lib.get_track(ids[0]).unwrap().content_hash;
        lib.clear_artwork(Some(&[folder_hash])).unwrap();
        assert_eq!(lib.artwork_bytes(None), 3);
        // The scoped removal also resets the duplicate track sharing the file.
        assert!(lib.get_track(ids[0]).unwrap().artwork_path.is_none());
        assert!(lib.get_track(shared).unwrap().artwork_path.is_none());
        assert!(lib.get_track(ids[1]).unwrap().artwork_path.is_some());
        lib.clear_artwork(None).unwrap();
        assert_eq!(lib.artwork_bytes(None), 0);
        assert!(lib.get_track(ids[1]).unwrap().artwork_path.is_none());
    }
}
