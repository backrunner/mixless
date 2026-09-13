//! Persistent verification hints. Content identity remains full-file BLAKE3;
//! filesystem revision metadata only permits reusing a previously computed hash.
use super::*;

fn fingerprint(path: &Path) -> Result<Option<String>, LibraryError> {
    let meta = fs::metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(Some(format!(
            "v1:{}:{}:{}:{}:{}:{}:{}",
            meta.dev(),
            meta.ino(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec()
        )))
    }
    #[cfg(not(unix))]
    {
        // Without a change timestamp, restored mtimes can conceal edits.
        let _ = meta;
        Ok(None)
    }
}

impl Library {
    /// Reuse verification across restarts only for the same filesystem revision.
    /// Never hold the database mutex while reading audio bytes.
    pub fn verified_content_hash(&self, path: &Path) -> Result<String, LibraryError> {
        let path = fs::canonicalize(path)?;
        let key = path.to_string_lossy();
        let before = fingerprint(&path)?;
        if let Some(ref revision) = before {
            let cached: Option<String> = self
                .conn
                .lock()
                .expect("library mutex")
                .query_row(
                    "SELECT content_hash FROM file_verification WHERE path=?1 AND fingerprint=?2",
                    params![key, revision],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(hash) = cached {
                return Ok(hash);
            }
        }
        let hash = content_hash(&path)?;
        let after = fingerprint(&path)?;
        if before != after {
            return Err(std::io::Error::other("File changed during verification; retry").into());
        }
        if let Some(revision) = after {
            self.conn.lock().expect("library mutex").execute(
                "INSERT INTO file_verification(path,fingerprint,content_hash) VALUES (?1,?2,?3)
                 ON CONFLICT(path) DO UPDATE SET fingerprint=excluded.fingerprint,content_hash=excluded.content_hash",
                params![key, revision, hash],
            )?;
        }
        Ok(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_survives_reopen_and_rejects_other_versions_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio");
        let db = dir.path().join("library.db");
        fs::write(&path, b"original").unwrap();
        let lib = Library::open(&db).unwrap();
        let id = lib.import_file(&path).unwrap();
        let hash = lib.verified_content_hash(&path).unwrap();
        let analysis = TrackAnalysis {
            track_id: id,
            duration_sec: 10.,
            sample_rate: 44100,
            tempo: mixless_protocol::TempoMap {
                global_bpm: 120.,
                ..Default::default()
            },
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
        };
        lib.finish_analysis(&analysis, &hash, 7).unwrap();
        drop(lib);
        let lib = Library::open(&db).unwrap();
        assert!(lib.get_track(id).unwrap().analyzed);
        assert_eq!(
            lib.load_analysis(id, 7).unwrap().unwrap().tempo.global_bpm,
            120.
        );
        assert!(lib.load_analysis(id, 8).unwrap().is_none());
        fs::write(&path, b"modified").unwrap();
        lib.import_file(&path).unwrap();
        assert!(!lib.get_track(id).unwrap().analyzed);
        assert!(lib.load_analysis(id, 7).unwrap().is_none());
    }

    #[test]
    fn verification_survives_reopen_and_invalidates_restored_mtime_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audio");
        let db = dir.path().join("library.db");
        fs::write(&path, b"original").unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let lib = Library::open(&db).unwrap();
        let original = lib.verified_content_hash(&path).unwrap();
        drop(lib);
        let lib = Library::open(&db).unwrap();
        assert_eq!(lib.verified_content_hash(&path).unwrap(), original);
        #[cfg(unix)]
        assert_eq!(
            lib.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM file_verification", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        fs::write(&path, b"modified").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_ne!(lib.verified_content_hash(&path).unwrap(), original);
        fs::remove_file(&path).unwrap();
        assert!(lib.verified_content_hash(&path).is_err());
    }
}
