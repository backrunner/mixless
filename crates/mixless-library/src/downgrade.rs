//! Read-only compatibility inspection and explicit, backed-up replacement.
//! Ordinary `Library::open` never performs this destructive conversion.
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, MAIN_DB};

use crate::{schema, Library, LibraryError, AUTO_CUE_VERSION, SCHEMA_VERSION, WAVEFORM_VERSION};

#[derive(Clone, Copy)]
pub struct Versions<'a> {
    pub analysis: u32,
    pub stems: u32,
    pub separator: &'a str,
    pub notes: &'a str,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Inspection {
    pub schema: u32,
    pub analysis: u32,
    pub waveform: u32,
    pub cues: u32,
    pub incompatible_stems: bool,
}

impl Inspection {
    pub fn needs_consent(&self, current: Versions<'_>) -> bool {
        self.schema > SCHEMA_VERSION
            || self.analysis > current.analysis
            || self.cues > AUTO_CUE_VERSION
            || self.waveform > WAVEFORM_VERSION
            || self.incompatible_stems
    }
}

fn read(path: &Path) -> Result<Connection, LibraryError> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?)
}

fn columns(conn: &Connection, db: &str, table: &str) -> Result<Vec<String>, LibraryError> {
    Ok(conn
        .prepare(&format!("PRAGMA {db}.table_info({table})"))?
        .query_map([], |r| r.get(1))?
        .collect::<Result<_, _>>()?)
}

fn maximum(conn: &Connection, table: &str) -> Result<u32, LibraryError> {
    if !columns(conn, "main", table)?.iter().any(|c| c == "version") {
        return Ok(0);
    }
    Ok(conn.query_row(
        &format!("SELECT COALESCE(MAX(version),0) FROM {table}"),
        [],
        |r| r.get(0),
    )?)
}

/// Does not open a normal Library, migrate, checkpoint or touch cache LRU times.
pub fn inspect(path: &Path, current: Versions<'_>) -> Result<Inspection, LibraryError> {
    if !path.exists() {
        return Ok(Inspection::default());
    }
    let conn = read(path)?;
    let mut result = Inspection {
        schema: schema::user_version(&conn)?,
        analysis: maximum(&conn, "track_analysis")?,
        waveform: maximum(&conn, "track_waveforms")?,
        cues: maximum(&conn, "cue_versions")?,
        ..Default::default()
    };
    if columns(&conn, "main", "track_analysis")?
        .iter()
        .any(|c| c == "payload")
    {
        // Inspect only generation metadata; future TrackAnalysis need not deserialize.
        result.incompatible_stems = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM track_analysis WHERE CASE WHEN json_valid(payload) THEN
              json_extract(payload,'$.stems.version') > ?1 OR
              (json_extract(payload,'$.stems.version') = ?1 AND
                (json_extract(payload,'$.stems.separator_sha256') != ?2 OR
                 json_extract(payload,'$.stems.notes_sha256') != ?3)) ELSE 0 END)",
            params![current.stems, current.separator, current.notes],
            |r| r.get(0),
        )?;
    }
    Ok(result)
}

/// A WAL-inclusive snapshot with a live source revision guard. Retain it
/// through activation so writes from legacy apps cannot be lost.
pub struct Snapshot {
    connection: Connection,
    revision: i64,
    path: PathBuf,
    source: PathBuf,
    #[cfg(unix)]
    identity: (u64, u64),
}

pub fn backup(path: &Path, destination: &Path) -> Result<Option<Snapshot>, LibraryError> {
    if !path.exists() {
        return Ok(None);
    }
    #[cfg(unix)]
    let identity = file_identity(path)?;
    let source = std::fs::canonicalize(path)?;
    let connection = read(path)?;
    let revision = data_version(&connection)?;
    connection.backup(MAIN_DB, destination, None)?;
    Ok(Some(Snapshot {
        connection,
        revision,
        path: destination.into(),
        source,
        #[cfg(unix)]
        identity,
    }))
}

fn data_version(connection: &Connection) -> Result<i64, LibraryError> {
    Ok(connection.pragma_query_value(None, "data_version", |r| r.get(0))?)
}

#[cfg(unix)]
fn file_identity(path: &Path) -> Result<(u64, u64), LibraryError> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path)?;
    Ok((meta.dev(), meta.ino()))
}

impl Snapshot {
    fn unchanged(&self, path: &Path) -> Result<(), LibraryError> {
        let same = self.source == std::fs::canonicalize(path)?
            && data_version(&self.connection)? == self.revision;
        #[cfg(unix)]
        let same = same && file_identity(path)? == self.identity;
        if !same {
            return Err(LibraryError::Io(std::io::Error::other(
                "The library changed during downgrade. Close other copies of mixless and try again.",
            )));
        }
        Ok(())
    }
}

/// Caller has obtained explicit consent and made `snapshot` with `backup`.
/// Build the entire supported schema separately, then activate with SQLite's
/// atomic backup transaction. Any copy/constraint failure leaves the source intact.
pub fn replace(
    path: &Path,
    snapshot: &Snapshot,
    current: Versions<'_>,
    replace_stems: bool,
) -> Result<(), LibraryError> {
    snapshot.unchanged(path)?;
    let staging = tempfile::tempdir_in(path.parent().unwrap_or(Path::new(".")))?;
    let staged_path = staging.path().join("library.db");
    let original = read(&snapshot.path)?;
    let newer = schema::user_version(&original)?;
    let rebuild = newer > SCHEMA_VERSION;
    let keep_derived = schema::check_compatible(&original, newer).is_ok();
    drop(original);
    if rebuild {
        // Unknown triggers and foreign keys must not delete user records
        // during cleanup, even when the future columns are additive.
        rebuild_schema(&staged_path, &snapshot.path, keep_derived)?;
    } else {
        backup(&snapshot.path, &staged_path)?;
        // The surface was checked before opening; enable this build's additive migrations.
        Connection::open(&staged_path)?.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    let library = Library::open(&staged_path)?;
    {
        let mut conn = library.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        // On a model downgrade, replace all embedded stem evidence. Otherwise
        // preserve analyses this build can read and regenerate newer rows only.
        tx.execute(
            "CREATE TEMP TABLE invalid_analysis AS SELECT track_id FROM track_analysis
            WHERE version > ?1 OR ?2 OR CASE WHEN json_valid(payload) THEN
              json_extract(payload,'$.stems.version') > ?3 OR
              (json_extract(payload,'$.stems.version') = ?3 AND
                (json_extract(payload,'$.stems.separator_sha256') != ?4 OR
                 json_extract(payload,'$.stems.notes_sha256') != ?5)) ELSE 0 END
            UNION SELECT track_id FROM cue_versions WHERE version > ?6",
            params![
                current.analysis,
                replace_stems,
                current.stems,
                current.separator,
                current.notes,
                AUTO_CUE_VERSION
            ],
        )?;
        tx.execute_batch("UPDATE tracks SET analyzed=0,bpm=NULL,key=NULL,camelot=NULL
                WHERE id IN (SELECT track_id FROM invalid_analysis);
            DELETE FROM cues WHERE user_set=0 AND track_id IN (SELECT track_id FROM invalid_analysis);
            DELETE FROM cue_versions WHERE track_id IN (SELECT track_id FROM invalid_analysis);
            DELETE FROM track_analysis WHERE track_id IN (SELECT track_id FROM invalid_analysis);
            DELETE FROM best_orders;
            DELETE FROM pair_costs;")?;
        tx.execute(
            "DELETE FROM track_waveforms WHERE version > ?1",
            [WAVEFORM_VERSION],
        )?;
        tx.commit()?;
    }
    drop(library);
    let mut to = Connection::open(path)?;
    to.busy_timeout(std::time::Duration::from_millis(100))?;
    let from = read(&staged_path)?;
    let activation = rusqlite::backup::Backup::new(&from, &mut to)?;
    // Taking the destination write lock BEFORE checking the source revision
    // closes the check/replace race, even with pre-protection older binaries.
    if !matches!(activation.step(0)?, rusqlite::backup::StepResult::More) {
        return Err(LibraryError::Io(std::io::Error::other(
            "Library is busy; close other copies of mixless and try again.",
        )));
    }
    snapshot.unchanged(path)?;
    if !matches!(activation.step(-1)?, rusqlite::backup::StepResult::Done) {
        return Err(LibraryError::Io(std::io::Error::other(
            "Library is busy; close other copies of mixless and try again.",
        )));
    }
    Ok(())
}

fn rebuild_schema(
    destination: &Path,
    snapshot: &Path,
    keep_derived: bool,
) -> Result<(), LibraryError> {
    let library = Library::open(destination)?;
    let mut conn = library.conn.lock().expect("library mutex");
    conn.execute(
        "ATTACH DATABASE ?1 AS previous",
        [snapshot.to_string_lossy().as_ref()],
    )?;
    conn.execute_batch("PRAGMA foreign_keys=OFF;")?;
    let tx = conn.transaction()?;
    for (table, required) in schema::REQUIRED_COLUMNS {
        if !keep_derived
            && [
                "track_analysis",
                "track_waveforms",
                "best_orders",
                "pair_costs",
                "cue_versions",
                "file_verification",
                "stem_identities",
            ]
            .contains(table)
        {
            continue;
        }
        let old = columns(&tx, "previous", table)?;
        // Never silently drop user-owned columns/tables we cannot map. Derived
        // track metadata is safe to discard, but paths, playlist IDs and manual
        // cue coordinates must retain their meaning.
        let retained: Vec<_> = required
            .iter()
            .filter(|c| {
                keep_derived
                    || !(*table == "tracks" && ["bpm", "key", "camelot", "analyzed"].contains(c))
            })
            .collect();
        if let Some(missing) = retained.iter().find(|c| !old.iter().any(|o| o == ***c)) {
            return Err(LibraryError::NewerSchema {
                found: schema::user_version(&read(snapshot)?)?,
                detail: format!("cannot preserve {table}.{missing}; use the latest mixless"),
            });
        }
        let names = retained
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(",");
        tx.execute(&format!("DELETE FROM {table}"), [])?;
        let filter = if *table == "cues" && !keep_derived {
            " WHERE user_set != 0"
        } else {
            ""
        };
        tx.execute(
            &format!("INSERT INTO {table} ({names}) SELECT {names} FROM previous.{table}{filter}"),
            [],
        )?;
    }
    if !columns(&tx, "previous", "sqlite_sequence")?.is_empty() {
        // Keep deleted IDs retired: library/package identities must not start
        // referring to a newly imported song after a schema conversion.
        tx.execute_batch("INSERT INTO sqlite_sequence(name,seq)
            SELECT name,seq FROM previous.sqlite_sequence AS old
            WHERE old.name IN ('tracks','playlists')
              AND NOT EXISTS (SELECT 1 FROM sqlite_sequence AS current WHERE current.name=old.name);
            UPDATE sqlite_sequence SET seq=MAX(seq,COALESCE(
              (SELECT seq FROM previous.sqlite_sequence AS old WHERE old.name=sqlite_sequence.name),0))
            WHERE name IN ('tracks','playlists');")?;
    }
    let violations: bool = tx.prepare("PRAGMA foreign_key_check")?.exists([])?;
    if violations {
        return Err(LibraryError::NewerSchema {
            found: SCHEMA_VERSION + 1,
            detail: "cannot preserve playlist or cue references; use the latest mixless".into(),
        });
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests;
