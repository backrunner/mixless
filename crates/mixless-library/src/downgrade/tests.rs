use super::*;

fn versions() -> Versions<'static> {
    Versions {
        analysis: 26,
        stems: 2,
        separator: "separator",
        notes: "notes",
    }
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let lib = Library::open(&path).unwrap();
    lib.conn.lock().unwrap().execute_batch(
        "INSERT INTO tracks(id,path,title,artist,duration_ms,content_hash,mtime,analyzed) VALUES(1,'/audio/song.wav','Song','Artist',10000,'hash',0,1);
         INSERT INTO playlists VALUES(1,'My set');
         INSERT INTO playlist_items VALUES(1,0,1);
         INSERT INTO cues VALUES(1,0,44100,'hot',1);
         INSERT INTO cues VALUES(1,1,88200,'hot',0);
         INSERT INTO track_analysis VALUES(1,'hash',999,'{}');
         INSERT INTO cue_versions VALUES(1,999);
         INSERT INTO best_orders VALUES('future','[0]',0);
         INSERT INTO track_waveforms VALUES(1,'hash',999,10,X'00');"
    ).unwrap();
    drop(lib);
    (dir, path)
}

#[test]
fn inspection_is_read_only_and_recognizes_all_derived_generations() {
    let (_dir, path) = fixture();
    let before = std::fs::read(&path).unwrap();
    let info = inspect(&path, versions()).unwrap();
    assert!(info.needs_consent(versions()));
    assert_eq!((info.analysis, info.cues, info.waveform), (999, 999, 999));
    assert_eq!(before, std::fs::read(&path).unwrap());
}

#[test]
fn replacement_preserves_user_data_and_backs_up_wal() {
    let (dir, path) = fixture();
    // Keep a live connection with uncheckpointed committed pages.
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
        UPDATE playlists SET name='Uncheckpointed'; PRAGMA user_version=999;
        ALTER TABLE tracks ADD COLUMN future TEXT;",
    )
    .unwrap();
    let saved = dir.path().join("backup.db");
    let snapshot = backup(&path, &saved).unwrap().unwrap();
    replace(&path, &snapshot, versions(), false).unwrap();
    let lib = Library::open(&path).unwrap();
    let current = lib.conn.lock().unwrap();
    assert_eq!(
        current
            .query_row("SELECT name FROM playlists WHERE id=1", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "Uncheckpointed"
    );
    assert_eq!(schema::user_version(&current).unwrap(), SCHEMA_VERSION);
    for table in ["tracks", "cues"] {
        assert_eq!(
            current
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    assert_eq!(
        current
            .query_row(
                "SELECT track_id FROM playlist_items WHERE playlist_id=1 AND position=0",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        current
            .query_row("SELECT analyzed FROM tracks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(!inspect(&path, versions())
        .unwrap()
        .needs_consent(versions()));
    assert_eq!(inspect(&saved, versions()).unwrap().analysis, 999);
}

#[test]
fn nonadditive_schema_is_converted_without_losing_playlists_or_manual_cues() {
    let (dir, path) = fixture();
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE tracks RENAME COLUMN bpm TO future_bpm; PRAGMA user_version=999;
             UPDATE sqlite_sequence SET seq=1000 WHERE name='tracks';",
        )
        .unwrap();
    let saved = dir.path().join("backup.db");
    let snapshot = backup(&path, &saved).unwrap().unwrap();
    replace(&path, &snapshot, versions(), false).unwrap();
    let lib = Library::open(&path).unwrap();
    let conn = lib.conn.lock().unwrap();
    assert_eq!(
        conn.query_row("SELECT path FROM tracks", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "/audio/song.wav"
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM cues WHERE user_set=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    conn.execute("INSERT INTO tracks(path,title,artist,duration_ms,content_hash,mtime) VALUES('/audio/new.wav','New','Artist',10000,'new',0)",[]).unwrap();
    assert_eq!(conn.last_insert_rowid(), 1001);
    drop(conn);
    assert_eq!(
        lib.list_playlists()
            .unwrap()
            .iter()
            .find(|p| p.id == 1)
            .unwrap()
            .name,
        "My set"
    );
}

#[test]
fn unconvertible_user_columns_fail_without_changing_source() {
    let (dir, path) = fixture();
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE tracks RENAME COLUMN path TO future_path; PRAGMA user_version=999;",
        )
        .unwrap();
    let saved = dir.path().join("backup.db");
    let snapshot = backup(&path, &saved).unwrap().unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(replace(&path, &snapshot, versions(), false).is_err());
    assert_eq!(before, std::fs::read(&path).unwrap());
    assert!(saved.exists());
}

#[test]
fn current_analysis_with_newer_model_evidence_requires_consent() {
    let (_dir, path) = fixture();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE track_analysis SET version=26,payload=?1",
        [r#"{"stems":{"version":2,"separator_sha256":"future","notes_sha256":"notes"}}"#],
    )
    .unwrap();
    conn.execute("DELETE FROM cue_versions", []).unwrap();
    conn.execute("DELETE FROM track_waveforms", []).unwrap();
    let info = inspect(&path, versions()).unwrap();
    assert!(info.incompatible_stems);
    assert!(info.needs_consent(versions()));
}

#[test]
fn concurrent_writer_causes_a_bounded_failure_without_replacing_the_library() {
    let (dir, path) = fixture();
    let saved = dir.path().join("backup.db");
    let snapshot = backup(&path, &saved).unwrap().unwrap();
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let before = std::fs::read(&path).unwrap();
    let started = std::time::Instant::now();
    assert!(replace(&path, &snapshot, versions(), false).is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    writer.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn automatic_cue_generation_has_its_own_version_gate() {
    let (dir, path) = fixture();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE track_analysis SET version=?1",
        [versions().analysis],
    )
    .unwrap();
    conn.execute("DELETE FROM track_waveforms", []).unwrap();
    conn.execute("UPDATE cue_versions SET version=?1", [AUTO_CUE_VERSION + 1])
        .unwrap();
    assert!(inspect(&path, versions())
        .unwrap()
        .needs_consent(versions()));
    let saved = dir.path().join("backup.db");
    let snapshot = backup(&path, &saved).unwrap().unwrap();
    replace(&path, &snapshot, versions(), false).unwrap();
    assert!(!inspect(&path, versions())
        .unwrap()
        .needs_consent(versions()));
    assert_eq!(
        Connection::open(&path)
            .unwrap()
            .query_row("SELECT count(*) FROM cues WHERE user_set=1", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn a_legacy_writer_committing_after_backup_is_never_overwritten() {
    let (dir, path) = fixture();
    let saved = dir.path().join("backup.db");
    let snapshot = backup(&path, &saved).unwrap().unwrap();
    let writer = Connection::open(&path).unwrap();
    writer
        .execute("UPDATE playlists SET name='Latest edit' WHERE id=1", [])
        .unwrap();
    let error = replace(&path, &snapshot, versions(), false).unwrap_err();
    assert!(error.to_string().contains("changed during downgrade"));
    assert_eq!(
        writer
            .query_row("SELECT name FROM playlists WHERE id=1", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "Latest edit"
    );
    assert_eq!(inspect(&path, versions()).unwrap().analysis, 999);
}

#[test]
fn the_activation_revision_check_is_protected_by_a_sqlite_write_lock() {
    let (dir, path) = fixture();
    let replacement_path = dir.path().join("replacement.db");
    drop(Library::open(&replacement_path).unwrap());
    let replacement = read(&replacement_path).unwrap();
    let mut destination = Connection::open(&path).unwrap();
    let activation = rusqlite::backup::Backup::new(&replacement, &mut destination).unwrap();
    assert!(matches!(
        activation.step(0).unwrap(),
        rusqlite::backup::StepResult::More
    ));
    let writer = Connection::open(&path).unwrap();
    writer
        .busy_timeout(std::time::Duration::from_millis(10))
        .unwrap();
    let error = writer
        .execute("UPDATE playlists SET name='Must not race'", [])
        .unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    // Dropping an unfinished activation rolls back, preserving the future library.
    drop(activation);
    assert_eq!(inspect(&path, versions()).unwrap().analysis, 999);
}

#[test]
fn unknown_future_triggers_cannot_delete_user_records_during_cache_replacement() {
    let (dir, path) = fixture();
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "PRAGMA user_version=999;
         CREATE TRIGGER future_cleanup AFTER DELETE ON track_analysis BEGIN
             DELETE FROM cues WHERE track_id=OLD.track_id;
             DELETE FROM tracks WHERE id=OLD.track_id;
         END;",
        )
        .unwrap();
    let saved = dir.path().join("backup.db");
    let snapshot = backup(&path, &saved).unwrap().unwrap();
    replace(&path, &snapshot, versions(), false).unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM tracks WHERE id=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM cues WHERE user_set=1", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
