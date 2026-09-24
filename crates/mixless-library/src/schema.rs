//! Database schema versioning for cross-version safety, including the
//! beta → stable downgrade path. `PRAGMA user_version` records which schema
//! generation last wrote the file, so an older build recognizes a newer
//! library before running any statement against it.
use rusqlite::Connection;

use super::LibraryError;

/// Bumped on every schema change.
///
/// Changes must stay additive — new tables, or new columns that are
/// nullable or carry a default. Every statement in this crate names its
/// columns explicitly, so a file holding extra newer-schema columns still
/// reads and writes correctly on an older build; payload versions gate the
/// derived data inside (`track_analysis`, `track_waveforms`, `cue_versions`).
/// A change that cannot be additive — rename, type change, NOT NULL without
/// default — makes older builds fail [`check_compatible`] and refuse the
/// file with [`LibraryError::NewerSchema`]; the app then sets the file
/// aside and starts a fresh library instead of corrupting rows or
/// crash-looping on every launch.
pub const SCHEMA_VERSION: u32 = 2;

/// Every column this build's statements name, per table. Extend the list
/// when a query starts using a new column so downgrades keep detecting
/// non-additive changes.
const REQUIRED_COLUMNS: &[(&str, &[&str])] = &[
    ("library_identity", &["singleton", "identity"]),
    ("package_tracks", &["source", "source_id", "track_id"]),
    (
        "tracks",
        &[
            "id",
            "path",
            "artwork_path",
            "title",
            "artist",
            "album",
            "duration_ms",
            "isrc",
            "bpm",
            "key",
            "camelot",
            "analyzed",
            "content_hash",
            "mtime",
        ],
    ),
    (
        "file_verification",
        &["path", "fingerprint", "content_hash"],
    ),
    ("cues", &["track_id", "idx", "frame", "kind", "user_set"]),
    (
        "track_analysis",
        &["track_id", "content_hash", "version", "payload"],
    ),
    (
        "track_waveforms",
        &["track_id", "content_hash", "version", "duration", "payload"],
    ),
    ("playlists", &["id", "name"]),
    ("playlist_items", &["playlist_id", "position", "track_id"]),
    ("playlist_exclusions", &["playlist_id", "track_id"]),
    ("library_order", &["track_id", "position"]),
    ("cue_versions", &["track_id", "version"]),
    (
        "external_playlists",
        &["source", "external_id", "playlist_id"],
    ),
    (
        "import_items",
        &[
            "playlist_id",
            "position",
            "external_id",
            "title",
            "artist",
            "duration_ms",
            "status",
            "track_id",
            "error",
        ],
    ),
    ("hidden_folder_playlists", &["folder_path"]),
];

/// Schema generation recorded on the file; 0 predates versioning.
/// `user_version` is a signed field, so a negative value reads as legacy.
pub fn user_version(conn: &Connection) -> Result<u32, LibraryError> {
    let stored: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    Ok(stored.max(0) as u32)
}

/// `stored` belongs to a newer build. The file is still openable when the
/// entire surface this build uses survived — extra tables and columns are
/// ignored. Anything missing, or an extra column our inserts cannot satisfy
/// (NOT NULL without a default — possible only via a table rebuild), means
/// the change was not additive, so refuse before a statement can misread or
/// partially write rows.
pub fn check_compatible(conn: &Connection, stored: u32) -> Result<(), LibraryError> {
    for (table, required) in REQUIRED_COLUMNS {
        let is_table: bool = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )?;
        if !is_table {
            // Missing entirely, or replaced by another object (a view would
            // pass the column check yet reject every write).
            return Err(LibraryError::NewerSchema {
                found: stored,
                detail: format!("missing table {table}"),
            });
        }
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        // (name, notnull, dflt_value) per column.
        let columns: Vec<(String, bool, Option<String>)> = stmt
            .query_map([], |row| {
                Ok((row.get(1)?, row.get::<_, i64>(3)? != 0, row.get(4)?))
            })?
            .filter_map(Result::ok)
            .collect();
        for column in *required {
            if !columns.iter().any(|(name, ..)| name == column) {
                return Err(LibraryError::NewerSchema {
                    found: stored,
                    detail: format!("missing column {table}.{column}"),
                });
            }
        }
        for (name, notnull, default) in &columns {
            if *notnull && default.is_none() && !required.contains(&name.as_str()) {
                return Err(LibraryError::NewerSchema {
                    found: stored,
                    detail: format!("{table}.{name} is NOT NULL without a default"),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Library;
    use mixless_protocol::TrackAnalysis;
    use std::fs;

    fn analysis(id: mixless_protocol::TrackId) -> TrackAnalysis {
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

    fn stored_version(library: &Library) -> u32 {
        library
            .conn
            .lock()
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_and_existing_libraries_carry_the_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("library.db");
        let lib = Library::open(&db).unwrap();
        assert_eq!(stored_version(&lib), SCHEMA_VERSION);
        drop(lib);
        // Reopening keeps the marker and the data.
        let lib = Library::open(&db).unwrap();
        assert_eq!(stored_version(&lib), SCHEMA_VERSION);
    }

    #[test]
    fn pre_versioning_library_is_migrated_and_stamped() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("library.db");
        {
            let lib = Library::open(&db).unwrap();
            // Simulate a file from before versioning existed: no marker and
            // no artwork column (the migration the first schema added).
            let conn = lib.conn.lock().unwrap();
            conn.execute_batch(
                "PRAGMA user_version = 0;
                 ALTER TABLE tracks DROP COLUMN artwork_path;",
            )
            .unwrap();
        }
        let lib = Library::open(&db).unwrap();
        assert_eq!(stored_version(&lib), SCHEMA_VERSION);
        let path = dir.path().join("song.wav");
        fs::write(&path, b"fixture").unwrap();
        lib.import_file(&path).unwrap();
    }

    #[test]
    fn newer_additive_schema_opens_and_derived_versions_still_gate() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("library.db");
        let track_path = dir.path().join("track.wav");
        fs::write(&track_path, b"fixture").unwrap();
        let id;
        {
            let lib = Library::open(&db).unwrap();
            id = lib.import_file(&track_path).unwrap();
            let hash = lib.get_track(id).unwrap().content_hash;
            // A "newer" build wrote an analysis payload at a version this
            // build does not know, marked the track analyzed, and added a
            // column this build never heard of.
            lib.finish_analysis(&analysis(id), &hash, 99).unwrap();
            let conn = lib.conn.lock().unwrap();
            conn.execute("ALTER TABLE tracks ADD COLUMN future_flag TEXT", [])
                .unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        let lib = Library::open(&db).unwrap();
        // The newer build's marker is preserved, never stamped down.
        assert_eq!(stored_version(&lib), SCHEMA_VERSION + 1);
        // Its payload is invisible here: the track re-analyzes instead of
        // the foreign format being decoded.
        assert!(lib.load_analysis(id, 7).unwrap().is_none());
        assert!(!lib.analysis_ready(id, 7).unwrap());
        // Ordinary writes still work against the additive surface.
        let other = dir.path().join("other.wav");
        fs::write(&other, b"second").unwrap();
        lib.import_file(&other).unwrap();
    }

    #[test]
    fn newer_schema_missing_a_required_column_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("library.db");
        {
            let lib = Library::open(&db).unwrap();
            let conn = lib.conn.lock().unwrap();
            conn.execute_batch(
                "ALTER TABLE tracks RENAME COLUMN bpm TO bpm_v2;
                 PRAGMA user_version = 999;",
            )
            .unwrap();
        }
        match Library::open(&db) {
            Err(LibraryError::NewerSchema { found, detail }) => {
                assert_eq!(found, 999);
                assert_eq!(detail, "missing column tracks.bpm");
            }
            other => panic!("expected NewerSchema, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn newer_schema_with_an_unsatisfiable_column_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("library.db");
        {
            let lib = Library::open(&db).unwrap();
            // A rebuilt table can carry a NOT NULL column with no default,
            // which `ALTER TABLE ADD COLUMN` alone cannot create. Every
            // column this build names still exists, yet our INSERTs would
            // fail at runtime — the check must catch it at open.
            let conn = lib.conn.lock().unwrap();
            conn.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 CREATE TABLE tracks_v2 (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     path TEXT NOT NULL UNIQUE,
                     artwork_path TEXT,
                     title TEXT NOT NULL,
                     artist TEXT NOT NULL,
                     album TEXT,
                     duration_ms INTEGER NOT NULL,
                     isrc TEXT,
                     bpm REAL,
                     key TEXT,
                     camelot TEXT,
                     analyzed INTEGER NOT NULL DEFAULT 0,
                     content_hash TEXT NOT NULL,
                     mtime INTEGER NOT NULL,
                     future_required TEXT NOT NULL
                 );
                 INSERT INTO tracks_v2 SELECT *, 'set' FROM tracks;
                 DROP TABLE tracks;
                 ALTER TABLE tracks_v2 RENAME TO tracks;
                 PRAGMA user_version = 999;",
            )
            .unwrap();
        }
        match Library::open(&db) {
            Err(LibraryError::NewerSchema { found, detail }) => {
                assert_eq!(found, 999);
                assert!(detail.contains("future_required"), "{detail}");
            }
            other => panic!("expected NewerSchema, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn newer_schema_missing_a_required_table_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("library.db");
        {
            let lib = Library::open(&db).unwrap();
            let conn = lib.conn.lock().unwrap();
            conn.execute_batch(
                "DROP TABLE cue_versions;
                 PRAGMA user_version = 999;",
            )
            .unwrap();
        }
        assert!(matches!(
            Library::open(&db),
            Err(LibraryError::NewerSchema { found: 999, .. })
        ));
    }
}
