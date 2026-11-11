use super::*;
use rusqlite::Connection;

fn future(data: &Path) {
    fs::create_dir_all(data).unwrap();
    let mut stamp = Stamp::current();
    stamp.app = "999.0.0".into();
    stamp.analysis += 1;
    atomic_json(&data.join(STAMP), &stamp).unwrap();
}

fn db(data: &Path) {
    let path = data.join("library.db");
    drop(mixless_library::Library::open(&path).unwrap());
    Connection::open(path).unwrap().execute_batch(
        "INSERT INTO tracks(id,path,title,artist,duration_ms,content_hash,mtime,analyzed) VALUES(1,'/music/song.wav','Song','Artist',10000,'hash',0,1);
        INSERT INTO playlists VALUES(1,'Saved set');
        INSERT INTO playlist_items VALUES(1,0,1);
        INSERT INTO track_analysis VALUES(1,'hash',999,'{}');").unwrap();
}

#[test]
fn declining_preserves_database_models_preferences_and_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    future(data);
    db(data);
    fs::create_dir(data.join("models")).unwrap();
    let model = data
        .join("models")
        .join(mixless_protocol::STEM_SEPARATOR.name);
    fs::write(&model, b"future model").unwrap();
    fs::write(
        data.join("preferences.json"),
        b"{\"stem_storage\":\"future\"}",
    )
    .unwrap();
    let paths = [
        data.join(STAMP),
        data.join("library.db"),
        data.join("preferences.json"),
        model,
    ];
    let before = paths
        .iter()
        .map(|p| fs::read(p).unwrap())
        .collect::<Vec<_>>();
    let session = prepare(data, &data.join("models"), |r| {
        assert!(r.needs_consent());
        false
    })
    .unwrap();
    assert!(session.is_none());
    for (path, bytes) in paths.iter().zip(before) {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert!(!data.join("downgrade-backups").exists());
}

#[test]
fn acceptance_replaces_future_data_preserves_library_and_does_not_prompt_again() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    future(data);
    db(data);
    let cache = data.join("stem-cache").join("a".repeat(64));
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("analysis.json"),
        r#"{"version":999,"separator_sha256":"new","notes_sha256":"new"}"#,
    )
    .unwrap();
    let models = data.join("models");
    fs::create_dir(&models).unwrap();
    fs::write(
        models.join(mixless_protocol::STEM_NOTES.name),
        b"future weights",
    )
    .unwrap();
    drop(prepare(data, &models, |_| true).unwrap().unwrap());
    assert!(!cache.exists());
    assert!(!models.join(mixless_protocol::STEM_NOTES.name).exists());
    let conn = Connection::open(data.join("library.db")).unwrap();
    assert_eq!(
        conn.query_row("SELECT name FROM playlists", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "Saved set"
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM track_analysis", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let backup = fs::read_dir(data.join("downgrade-backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::read(
            backup
                .join("models")
                .join(mixless_protocol::STEM_NOTES.name)
        )
        .unwrap(),
        b"future weights"
    );
    assert!(backup.join("library.db").exists());
    drop(
        prepare(data, &models, |_| {
            panic!("accepted downgrade prompted again")
        })
        .unwrap(),
    );
}

#[test]
fn first_launch_and_normal_upgrade_do_not_prompt() {
    let dir = tempfile::tempdir().unwrap();
    drop(
        prepare(dir.path(), &dir.path().join("models"), |_| {
            panic!("first launch")
        })
        .unwrap(),
    );
    let mut stamp = Stamp::current();
    stamp.app = "0.0.1".into();
    stamp.built = "2000-01-01T00:00:00Z".into();
    atomic_json(&dir.path().join(STAMP), &stamp).unwrap();
    drop(
        prepare(dir.path(), &dir.path().join("models"), |_| {
            panic!("upgrade")
        })
        .unwrap(),
    );
}

#[test]
fn newer_analysis_without_a_ledger_still_requires_consent() {
    let dir = tempfile::tempdir().unwrap();
    db(dir.path());
    assert!(
        prepare(dir.path(), &dir.path().join("models"), |r| {
            assert_eq!(r.library.analysis, 999);
            false
        })
        .unwrap()
        .is_none()
    );
}

#[test]
fn unknown_preferences_require_consent_and_preserve_compatible_fields() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("preferences.json"),
        r#"{"show_fx":false,"stem_storage":"future"}"#,
    )
    .unwrap();
    drop(
        prepare(dir.path(), &dir.path().join("models"), |r| {
            assert!(r.reset_preferences);
            true
        })
        .unwrap(),
    );
    let settings: crate::settings::Settings =
        serde_json::from_slice(&fs::read(dir.path().join("preferences.json")).unwrap()).unwrap();
    assert!(!settings.show_fx);
    settings.validate().unwrap();
}

#[test]
fn changed_data_and_running_copies_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    future(dir.path());
    let result = prepare(dir.path(), &dir.path().join("models"), |_| {
        let mut stamp = Stamp::current();
        stamp.app = "1000.0.0".into();
        atomic_json(&dir.path().join(STAMP), &stamp).unwrap();
        true
    });
    assert!(result.err().unwrap().contains("changed"));
    let lease = lock(&dir.path().join(".compatibility.lock")).unwrap();
    assert!(
        prepare(dir.path(), &dir.path().join("models"), |_| panic!(
            "should not prompt"
        ))
        .is_err()
    );
    drop(lease);
}

#[test]
fn active_inference_blocks_replacement() {
    let dir = tempfile::tempdir().unwrap();
    future(dir.path());
    let path = dir.path().join("stem-cache/.analysis.lock");
    let lease = lock(&path).unwrap();
    assert!(prepare(dir.path(), &dir.path().join("models"), |_| true).is_err());
    assert!(!dir.path().join("downgrade-backups").exists());
    drop(lease);
}

#[test]
fn failed_schema_conversion_restores_models_and_original_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    future(data);
    db(data);
    Connection::open(data.join("library.db"))
        .unwrap()
        .execute_batch(
            "ALTER TABLE tracks RENAME COLUMN path TO future_path; PRAGMA user_version=999;",
        )
        .unwrap();
    let prefs = br#"{"show_fx":false,"stem_storage":"future"}"#;
    fs::write(data.join("preferences.json"), prefs).unwrap();
    let models = data.join("models");
    fs::create_dir(&models).unwrap();
    let model = models.join(mixless_protocol::STEM_NOTES.name);
    fs::write(&model, b"future model").unwrap();
    let before = fs::read(data.join("library.db")).unwrap();
    assert!(prepare(data, &models, |_| true).is_err());
    assert_eq!(fs::read(data.join("library.db")).unwrap(), before);
    assert_eq!(fs::read(data.join("preferences.json")).unwrap(), prefs);
    assert_eq!(fs::read(model).unwrap(), b"future model");
    assert_eq!(
        inspect(data, &models).unwrap().previous.unwrap().app,
        "999.0.0"
    );
}

#[test]
fn unknown_cache_format_and_manifest_shape_require_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    for (key, payload) in [
        (
            'a',
            serde_json::json!({"version":mixless_stems::VERSION,"separator_sha256":mixless_protocol::STEM_SEPARATOR_HASH,"notes_sha256":mixless_protocol::STEM_NOTES.sha256,"storage":{"version":1,"format":"future"}}),
        ),
        ('b', serde_json::json!({"new_manifest":{"version":999}})),
    ] {
        let path = dir
            .path()
            .join("stem-cache")
            .join(key.to_string().repeat(64));
        fs::create_dir_all(&path).unwrap();
        atomic_json(&path.join("analysis.json"), &payload).unwrap();
    }
    let report = inspect(dir.path(), &dir.path().join("models")).unwrap();
    assert_eq!(report.assets.incompatible_stems.len(), 2);
    assert!(report.needs_consent());
}

#[test]
fn model_paths_in_foreign_ledger_cannot_escape_the_model_directory() {
    let dir = tempfile::tempdir().unwrap();
    let sibling = dir.path().join("keep.onnx");
    fs::write(&sibling, b"keep").unwrap();
    let mut stamp = Stamp::current();
    stamp.models.push(assets::ModelFile {
        name: "../keep.onnx".into(),
        sha256: "future".into(),
        bytes: 4,
        modified: 0,
    });
    fs::create_dir(dir.path().join("models")).unwrap();
    fs::write(dir.path().join("models/.models.lock"), b"lock").unwrap();
    stamp.models.push(assets::ModelFile {
        name: ".models.lock".into(),
        sha256: "future".into(),
        bytes: 4,
        modified: 0,
    });
    let result = assets::inspect(dir.path(), &dir.path().join("models"), Some(&stamp)).unwrap();
    assert!(result.incompatible_models.is_empty());
    assert_eq!(fs::read(sibling).unwrap(), b"keep");
}
