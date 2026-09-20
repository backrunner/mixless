//! Local library: import, metadata, cues. No tokens, no PCM.

mod cues;
mod editing;
mod folders;
mod hash;
mod imports;
mod ordering;
mod schema;
mod spotify_artwork;
mod storage;
mod verification;
mod waveform;
pub use imports::ImportItem;
pub use schema::SCHEMA_VERSION;
pub use spotify_artwork::SpotifyArtworkTarget;
pub use storage::CacheUsage;

use std::fs;
use std::path::{Path, PathBuf};

use lofty::file::AudioFile;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey, TaggedFileExt};
use lofty::probe::Probe;
use mixless_protocol::{Cue, CueKind, PlaylistId, Track, TrackAnalysis, TrackId};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use thiserror::Error;

pub use hash::content_hash;

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
    pub tracks: u32,
    pub folder_path: Option<String>,
    pub failed_imports: u32,
}

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sql: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("not found")]
    NotFound,
    #[error("Track order changed; refresh the list and try again")]
    OrderChanged,
    #[error("invalid cue index {0}")]
    CueIndex(u8),
    #[error("analysis cache: {0}")]
    AnalysisJson(#[from] serde_json::Error),
    /// A newer build wrote this file and the change was not additive; the
    /// caller sets it aside rather than opening it.
    #[error("library was written by a newer app (schema v{found}): {detail}")]
    NewerSchema { found: u32, detail: String },
}

pub struct Library {
    conn: std::sync::Mutex<Connection>,
    db_path: PathBuf,
    artwork_dir: PathBuf,
}

impl Library {
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        // Read the marker before touching the file: a refused library is
        // left byte-identical, including its journal mode.
        let stored = schema::user_version(&conn)?;
        if stored > SCHEMA_VERSION {
            // Beta → stable downgrade: the file belongs to a newer schema
            // generation. It stays openable while the whole surface this
            // build uses survived — payload versions still gate the derived
            // data inside — and the newer build's marker is never stamped
            // down. Otherwise refuse before any statement can misread rows.
            schema::check_compatible(&conn, stored)?;
        }
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            ",
        )?;
        if stored <= SCHEMA_VERSION {
            conn.execute_batch(
                "
            CREATE TABLE IF NOT EXISTS tracks (
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
                mtime INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS file_verification (
                path TEXT PRIMARY KEY,
                fingerprint TEXT NOT NULL,
                content_hash TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cues (
                track_id INTEGER NOT NULL,
                idx INTEGER NOT NULL,
                frame INTEGER NOT NULL,
                kind TEXT NOT NULL,
                user_set INTEGER NOT NULL,
                PRIMARY KEY (track_id, idx),
                FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS track_analysis (
                track_id INTEGER PRIMARY KEY,
                content_hash TEXT NOT NULL,
                version INTEGER NOT NULL,
                payload TEXT NOT NULL,
                FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS track_waveforms (
                track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
                content_hash TEXT NOT NULL,
                version INTEGER NOT NULL,
                duration REAL NOT NULL,
                payload BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS playlists (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS playlist_items (
                playlist_id INTEGER NOT NULL,
                position INTEGER NOT NULL,
                track_id INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, position),
                FOREIGN KEY (playlist_id) REFERENCES playlists(id) ON DELETE CASCADE,
                FOREIGN KEY (track_id) REFERENCES tracks(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS playlist_exclusions (
                playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                PRIMARY KEY (playlist_id, track_id)
            );
            CREATE TABLE IF NOT EXISTS library_order (
                track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
                position INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cue_versions (
                track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
                version INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS external_playlists (
                source TEXT NOT NULL,
                external_id TEXT NOT NULL,
                playlist_id INTEGER NOT NULL UNIQUE REFERENCES playlists(id) ON DELETE CASCADE,
                PRIMARY KEY (source, external_id)
            );
            CREATE TABLE IF NOT EXISTS import_items (
                playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                position INTEGER NOT NULL,
                external_id TEXT NOT NULL,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                status TEXT NOT NULL,
                track_id INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
                error TEXT,
                PRIMARY KEY (playlist_id, position)
            );
            CREATE TABLE IF NOT EXISTS hidden_folder_playlists (
                folder_path TEXT PRIMARY KEY
            );
            ",
            )?;
            // `CREATE TABLE IF NOT EXISTS` does not add columns to libraries
            // made by earlier builds. Keep the migration local and backwards
            // compatible.
            let has_artwork_path = {
                let mut stmt = conn.prepare("PRAGMA table_info(tracks)")?;
                let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
                let found = columns
                    .filter_map(Result::ok)
                    .any(|name| name == "artwork_path");
                found
            };
            if !has_artwork_path {
                conn.execute("ALTER TABLE tracks ADD COLUMN artwork_path TEXT", [])?;
            }
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        let artwork_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("artwork");
        let library = Self {
            conn: std::sync::Mutex::new(conn),
            db_path: path.to_path_buf(),
            artwork_dir,
        };
        library.backfill_folder_playlists(&path.with_file_name("acquired"))?;
        Ok(library)
    }

    pub fn import_file(&self, path: &Path) -> Result<TrackId, LibraryError> {
        let canon = fs::canonicalize(path)?;
        let path_str = canon.to_string_lossy().to_string();
        let hash = self.verified_content_hash(&canon)?;
        let meta = read_meta(&canon);
        let artwork_path = self.cache_artwork(&hash, meta.artwork.as_ref())?;
        // Empty means "checked, no embedded artwork". It avoids reparsing the
        // same coverless file on every launch; row mapping turns it back into
        // `None` for UI callers.
        let stored_artwork_path = artwork_path.clone().unwrap_or_default();
        let mtime = fs::metadata(&canon)?
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let conn = self.conn.lock().expect("library mutex");
        conn.execute(
            "INSERT INTO tracks (path, artwork_path, title, artist, album, duration_ms, isrc, content_hash, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(path) DO UPDATE SET
                artwork_path=CASE
                    WHEN tracks.content_hash=excluded.content_hash AND excluded.artwork_path=''
                    THEN tracks.artwork_path ELSE excluded.artwork_path END,
                title=excluded.title,
                artist=excluded.artist,
                album=excluded.album,
                duration_ms=excluded.duration_ms,
                isrc=excluded.isrc,
                bpm=CASE WHEN tracks.content_hash=excluded.content_hash THEN tracks.bpm ELSE NULL END,
                key=CASE WHEN tracks.content_hash=excluded.content_hash THEN tracks.key ELSE NULL END,
                camelot=CASE WHEN tracks.content_hash=excluded.content_hash THEN tracks.camelot ELSE NULL END,
                analyzed=CASE WHEN tracks.content_hash=excluded.content_hash THEN tracks.analyzed ELSE 0 END,
                content_hash=excluded.content_hash,
                mtime=excluded.mtime",
            params![
                path_str,
                stored_artwork_path,
                meta.title,
                meta.artist,
                meta.album,
                meta.duration_ms as i64,
                meta.isrc,
                hash,
                mtime
            ],
        )?;
        let id = conn.query_row(
            "SELECT id FROM tracks WHERE path = ?1",
            params![path_str],
            |r| r.get(0),
        )?;
        Ok(TrackId(id))
    }

    pub fn get_track(&self, id: TrackId) -> Result<Track, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        conn.query_row(
            "SELECT id, path, artwork_path, title, artist, album, duration_ms, isrc, bpm, key, camelot, analyzed, content_hash
             FROM tracks WHERE id = ?1",
            params![id.0],
            row_to_track,
        )
        .optional()?
        .ok_or(LibraryError::NotFound)
    }

    pub fn list_tracks(&self) -> Result<Vec<Track>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let mut stmt = conn.prepare(
            "SELECT t.id, t.path, t.artwork_path, t.title, t.artist, t.album, t.duration_ms,
                    t.isrc, t.bpm, t.key, t.camelot, t.analyzed, t.content_hash
             FROM tracks t LEFT JOIN library_order o ON o.track_id=t.id
             ORDER BY o.position IS NULL, o.position, t.artist, t.title, t.id",
        )?;
        let rows = stmt.query_map([], row_to_track)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn track_at_path(&self, path: &Path) -> Result<Option<Track>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        Ok(conn.query_row(
            "SELECT id,path,artwork_path,title,artist,album,duration_ms,isrc,bpm,key,camelot,analyzed,content_hash
             FROM tracks WHERE path=?1",
            [path.to_string_lossy().as_ref()], row_to_track,
        ).optional()?)
    }

    pub fn set_cue(
        &self,
        id: TrackId,
        index: u8,
        frame: u64,
        kind: CueKind,
        user_set: bool,
    ) -> Result<(), LibraryError> {
        if index >= 8 {
            return Err(LibraryError::CueIndex(index));
        }
        let kind_s = match kind {
            CueKind::Hot => "hot",
            CueKind::In => "in",
            CueKind::Out => "out",
        };
        let conn = self.conn.lock().expect("library mutex");
        conn.execute(
            "INSERT INTO cues (track_id, idx, frame, kind, user_set)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(track_id, idx) DO UPDATE SET
                frame=excluded.frame, kind=excluded.kind, user_set=excluded.user_set",
            params![id.0, index as i64, frame as i64, kind_s, user_set as i64],
        )?;
        Ok(())
    }

    pub fn cues(&self, id: TrackId) -> Result<Vec<Cue>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let mut stmt = conn.prepare(
            "SELECT idx, frame, kind, user_set FROM cues WHERE track_id = ?1 ORDER BY idx",
        )?;
        let rows = stmt.query_map(params![id.0], |r| {
            let kind_s: String = r.get(2)?;
            let kind = match kind_s.as_str() {
                "in" => CueKind::In,
                "out" => CueKind::Out,
                _ => CueKind::Hot,
            };
            Ok(Cue {
                index: r.get::<_, i64>(0)? as u8,
                frame: r.get::<_, i64>(1)? as u64,
                kind,
                user_set: r.get::<_, i64>(3)? != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn set_analysis_meta(
        &self,
        id: TrackId,
        bpm: Option<f32>,
        key: Option<&str>,
        camelot: Option<&str>,
    ) -> Result<(), LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        conn.execute(
            "UPDATE tracks SET bpm=?1, key=?2, camelot=?3, analyzed=1 WHERE id=?4",
            params![bpm, key, camelot, id.0],
        )?;
        Ok(())
    }

    /// Only cache results belonging to the current file revision.
    pub fn save_analysis(
        &self,
        analysis: &TrackAnalysis,
        content_hash: &str,
        version: u32,
    ) -> Result<(), LibraryError> {
        let payload = serde_json::to_string(analysis)?;
        let conn = self.conn.lock().expect("library mutex");
        let count=conn.execute("INSERT INTO track_analysis (track_id,content_hash,version,payload)
            SELECT id,content_hash,?3,?4 FROM tracks WHERE id=?1 AND content_hash=?2
            ON CONFLICT(track_id) DO UPDATE SET content_hash=excluded.content_hash,version=excluded.version,payload=excluded.payload",
            params![analysis.track_id.0,content_hash,version,payload])?;
        if count == 0 {
            return Err(LibraryError::NotFound);
        }
        Ok(())
    }

    pub fn load_analysis(
        &self,
        id: TrackId,
        version: u32,
    ) -> Result<Option<TrackAnalysis>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let payload: Option<String> = conn
            .query_row(
                "SELECT a.payload FROM track_analysis a JOIN tracks t ON a.track_id=t.id
            WHERE a.track_id=?1 AND a.version=?2 AND a.content_hash=t.content_hash",
                params![id.0, version],
                |r| r.get(0),
            )
            .optional()?;
        drop(conn);
        payload
            .map(|json| serde_json::from_str(&json).map_err(LibraryError::from))
            .transpose()
    }

    /// A track is playable only when its row and cached payload describe the
    /// current file revision. Verification reuses the persistent filesystem
    /// revision cache, including change time so restored mtimes invalidate it.
    pub fn analysis_ready(&self, id: TrackId, version: u32) -> Result<bool, LibraryError> {
        let track = self.get_track(id)?;
        if !track.analyzed {
            return Ok(false);
        }
        let current = self.verified_content_hash(Path::new(&track.path))?;
        if current != track.content_hash {
            return Ok(false);
        }
        Ok(self.load_analysis(id, version)?.is_some())
    }

    /// Invalidate metadata and cache atomically before analyzing a file revision.
    /// Cues and playlist membership remain attached to the track identity.
    pub fn begin_analysis(&self, id: TrackId, hash: &str) -> Result<(), LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        if tx.execute("UPDATE tracks SET analyzed=0,bpm=NULL,key=NULL,camelot=NULL,content_hash=?2 WHERE id=?1",
            params![id.0, hash])? != 1 {
            return Err(LibraryError::NotFound);
        }
        tx.execute("DELETE FROM track_analysis WHERE track_id=?1", [id.0])?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_playlists(&self) -> Result<Vec<PlaylistSummary>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, COUNT(i.track_id) +
                    (SELECT COUNT(*) FROM import_items missing WHERE missing.playlist_id=p.id
                     AND (missing.status NOT IN ('local','acquired') OR missing.track_id IS NULL)) AS n,
                    CASE WHEN e.source='folder' THEN e.external_id END AS folder_path,
                    (SELECT COUNT(*) FROM import_items failed WHERE failed.playlist_id=p.id
                     AND e.source='spotify' AND failed.status IN ('missing','suspect'))
             FROM playlists p
             LEFT JOIN playlist_items i ON i.playlist_id = p.id
             LEFT JOIN external_playlists e ON e.playlist_id = p.id
             GROUP BY p.id
             ORDER BY folder_path IS NULL, p.name, folder_path, p.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(PlaylistSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                tracks: r.get::<_, i64>(2)? as u32,
                folder_path: r.get(3)?,
                failed_imports: r.get::<_, i64>(4)? as u32,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn playlist_tracks(&self, id: PlaylistId) -> Result<Vec<Track>, LibraryError> {
        let conn = self.conn.lock().expect("library mutex");
        let mut stmt = conn.prepare(
            "SELECT t.id, t.path, t.artwork_path, t.title, t.artist, t.album, t.duration_ms, t.isrc,
                    t.bpm, t.key, t.camelot, t.analyzed, t.content_hash
             FROM playlist_items i
             JOIN tracks t ON t.id = i.track_id
             WHERE i.playlist_id = ?1
             ORDER BY i.position",
        )?;
        let rows = stmt.query_map(params![id.0], row_to_track)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Refresh membership, preserving existing manual order and appending new songs.
    pub fn replace_playlist(
        &self,
        name: &str,
        track_ids: &[TrackId],
    ) -> Result<PlaylistId, LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let existing: Option<i64> = tx.query_row(
            "SELECT id FROM playlists WHERE name=?1 AND id NOT IN (SELECT playlist_id FROM external_playlists) LIMIT 1",
            [name], |r| r.get(0)).optional()?;
        let id = if let Some(id) = existing {
            id
        } else {
            tx.execute("INSERT INTO playlists (name) VALUES (?1)", [name])?;
            tx.last_insert_rowid()
        };
        let ordered = ordering::refreshed_order(&tx, PlaylistId(id), track_ids)?;
        tx.execute("DELETE FROM playlist_items WHERE playlist_id=?1", [id])?;
        for (pos, tid) in ordered.iter().enumerate() {
            tx.execute(
                "INSERT INTO playlist_items (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
                params![id, pos as i64, tid.0],
            )?;
        }
        tx.commit()?;
        Ok(PlaylistId(id))
    }

    /// Populate cover art for tracks imported before artwork caching existed.
    /// Callers run this on a worker thread; tag parsing never blocks rendering.
    pub fn backfill_artwork(&self) -> Result<usize, LibraryError> {
        let missing = {
            let conn = self.conn.lock().expect("library mutex");
            let mut stmt = conn
                .prepare("SELECT id, path, content_hash FROM tracks WHERE artwork_path IS NULL")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.filter_map(Result::ok).collect::<Vec<_>>()
        };

        let mut updated = 0;
        for (id, path, hash) in missing {
            let meta = read_meta(Path::new(&path));
            let artwork_path = self
                .cache_artwork(&hash, meta.artwork.as_ref())?
                .unwrap_or_default();
            let conn = self.conn.lock().expect("library mutex");
            let changed = conn.execute(
                "UPDATE tracks SET artwork_path = ?1 WHERE id = ?2 AND content_hash=?3
                 AND artwork_path IS NULL",
                params![artwork_path, id, hash],
            )?;
            updated += changed * usize::from(!artwork_path.is_empty());
        }
        Ok(updated)
    }

    fn cache_artwork(
        &self,
        content_hash: &str,
        artwork: Option<&Artwork>,
    ) -> Result<Option<String>, LibraryError> {
        let Some(artwork) = artwork else {
            return Ok(None);
        };
        fs::create_dir_all(&self.artwork_dir)?;
        let path = self
            .artwork_dir
            .join(format!("{content_hash}.{}", artwork.extension));
        if !path.exists() {
            use std::io::Write;
            let mut pending = tempfile::NamedTempFile::new_in(&self.artwork_dir)?;
            pending.write_all(&artwork.data)?;
            if let Err(error) = pending.persist_noclobber(&path) {
                if error.error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error.error.into());
                }
            }
        }
        Ok(Some(path.to_string_lossy().into_owned()))
    }
}

fn row_to_track(r: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: TrackId(r.get(0)?),
        path: r.get(1)?,
        artwork_path: r
            .get::<_, Option<String>>(2)?
            .filter(|path| !path.is_empty()),
        title: r.get(3)?,
        artist: r.get(4)?,
        album: r.get(5)?,
        duration_ms: r.get::<_, i64>(6)? as u32,
        isrc: r.get(7)?,
        bpm: r.get(8)?,
        key: r.get(9)?,
        camelot: r.get(10)?,
        analyzed: r.get::<_, i64>(11)? != 0,
        content_hash: r.get(12)?,
    })
}

struct Artwork {
    data: Vec<u8>,
    extension: &'static str,
}

struct FileMeta {
    title: String,
    artist: String,
    album: Option<String>,
    duration_ms: u32,
    isrc: Option<String>,
    artwork: Option<Artwork>,
}

fn read_meta(path: &Path) -> FileMeta {
    let fallback = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();
    let tagged = Probe::open(path).ok().and_then(|p| p.read().ok());
    let Some(tagged) = tagged else {
        return FileMeta {
            title: fallback,
            artist: "Unknown".into(),
            album: None,
            duration_ms: 0,
            isrc: None,
            artwork: None,
        };
    };
    let duration_ms = tagged.properties().duration().as_millis() as u32;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let title = tag
        .and_then(|t| t.title().map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback);
    let artist = tag
        .and_then(|t| t.artist().map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown".into());
    let album = tag.and_then(|t| t.album().map(|s| s.to_string()));
    let isrc = tag.and_then(|t| {
        t.get_string(&ItemKey::Isrc)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    });
    let artwork = tag.and_then(|tag| {
        tag.get_picture_type(PictureType::CoverFront)
            .or_else(|| tag.pictures().first())
            .and_then(artwork_from_picture)
    });
    FileMeta {
        title,
        artist,
        album,
        duration_ms,
        isrc,
        artwork,
    }
}

fn artwork_from_picture(picture: &Picture) -> Option<Artwork> {
    if picture.data().is_empty() {
        return None;
    }
    let extension = match picture.mime_type() {
        Some(MimeType::Jpeg) => "jpg",
        Some(MimeType::Png) => "png",
        Some(MimeType::Gif) => "gif",
        Some(MimeType::Bmp) => "bmp",
        Some(MimeType::Tiff) => "tif",
        _ => sniff_image_extension(picture.data())?,
    };
    Some(Artwork {
        data: picture.data().to_vec(),
        extension,
    })
}

fn sniff_image_extension(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("gif")
    } else if data.starts_with(b"BM") {
        Some("bmp")
    } else if data.starts_with(b"II*\0") || data.starts_with(b"MM\0*") {
        Some("tif")
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        Some("webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_import_missing_is_io() {
        let dir = std::env::temp_dir().join(format!("mixless-lib-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let lib = Library::open(&dir.join("lib.db")).unwrap();
        assert!(lib.import_file(Path::new("/no/such/file.wav")).is_err());
        assert!(lib.list_tracks().unwrap().is_empty());
    }

    #[test]
    fn open_migrates_legacy_track_table_for_artwork() {
        let dir = std::env::temp_dir().join(format!("mixless-lib-migrate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lib.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tracks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
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
                mtime INTEGER NOT NULL
            );",
        )
        .unwrap();
        drop(conn);

        let lib = Library::open(&path).unwrap();
        let conn = lib.conn.lock().unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(tracks)").unwrap();
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(names.iter().any(|name| name == "artwork_path"));
    }
}
