use super::*;
use std::fs;

fn manager(dir: &Path) -> ControllerManager {
    ControllerManager::start_with_config(
        dir.join("midi.json"),
        MidiConfig {
            enabled: false,
            input_ids: None,
        },
    )
    .unwrap()
}
fn write(dir: &Path, name: &str, p: &ControllerPreset) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, serde_json::to_vec(p).unwrap()).unwrap();
    path
}
fn ready(batch: &ImportBatch) -> Vec<ImportReport> {
    batch
        .entries
        .iter()
        .filter_map(|e| e.result.as_ref().ok())
        .cloned()
        .collect()
}

#[test]
fn batch_review_preserves_every_file_and_reports_existing_and_internal_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let hub = manager(dir.path());
    let p = builtin_presets().remove(0);
    let a = write(dir.path(), "a.json", &p);
    let b = write(dir.path(), "b.json", &p);
    let bad = dir.path().join("bad.json");
    fs::write(&bad, "{ invalid JSON").unwrap();
    let mut newer = p.clone();
    newer.version = 999;
    let newer = write(dir.path(), "newer.json", &newer);
    let before = hub.presets();
    let batch = hub.preview_imports(&[a, b, bad, newer]).unwrap();
    assert_eq!(batch.ready_count(), 2);
    assert_eq!(batch.skipped_count(), 2);
    assert_eq!(
        batch
            .entries
            .iter()
            .map(|e| e.file_name.as_str())
            .collect::<Vec<_>>(),
        ["a.json", "b.json", "bad.json", "newer.json"]
    );
    for report in ready(&batch) {
        assert!(
            report
                .conflicts
                .iter()
                .any(|c| c.contains("already exists"))
        );
        assert!(report.conflicts.iter().any(|c| c.contains("selected file")));
    }
    assert!(batch.entries[2].result.is_err());
    assert!(
        batch.entries[3]
            .result
            .as_ref()
            .unwrap_err()
            .contains("version")
    );
    assert_eq!(hub.presets(), before);
    assert!(!hub.storage_path().exists());
}

#[test]
fn accepting_a_batch_keeps_names_assignments_and_bindings_and_persists_unique_copies() {
    let dir = tempfile::tempdir().unwrap();
    let hub = manager(dir.path());
    let p = builtin_presets().remove(0);
    let mut q = p.clone();
    q.verification = Verification::Hardware;
    let a = write(dir.path(), "a.json", &p);
    let b = write(dir.path(), "b.json", &q);
    hub.assign(DeviceAssignment {
        device_id: "offline-unit".into(),
        preset_id: p.id.clone(),
        ports: BTreeMap::from([("main".into(), "offline-unit".into())]),
        outputs: Default::default(),
    })
    .unwrap();
    let assignments = hub.assignments();
    let bindings = hub.bindings();
    let count = hub.presets().len();
    let batch = hub.preview_imports(&[a, b]).unwrap();
    let ids = hub.accept_imports(ready(&batch)).unwrap();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    for id in &ids {
        let imported = hub.preset(id).unwrap();
        assert!(!imported.is_builtin());
        assert_eq!(imported.name, p.name);
        assert_eq!(imported.verification, Verification::Protocol);
        assert_eq!(imported.bindings, p.bindings);
    }
    assert_eq!(hub.presets().len(), count + 2);
    assert_eq!(hub.assignments(), assignments);
    assert_eq!(hub.bindings(), bindings);
    drop(hub);
    let reopened = manager(dir.path());
    assert_eq!(reopened.presets().len(), count + 2);
    assert_eq!(reopened.assignments(), assignments);
    assert!(ids.iter().all(|id| reopened.preset(id).is_some()));
}

#[test]
fn invalid_later_report_or_adapter_rolls_back_the_whole_batch() {
    let dir = tempfile::tempdir().unwrap();
    let hub = manager(dir.path());
    let p = builtin_presets().remove(0);
    let path = write(dir.path(), "a.json", &p);
    let report = hub.preview_import(&path).unwrap();
    let before = hub.presets();
    let mut bad = report.clone();
    bad.preset.name.clear();
    assert!(hub.accept_imports(vec![report.clone(), bad]).is_err());
    assert_eq!(hub.presets(), before);
    let mut adapter: ControllerPreset = serde_json::from_str(include_str!(
        "../../../examples/synthetic-hid/preset.template.json"
    ))
    .unwrap();
    adapter.adapter.as_mut().unwrap().set_module(&wat::parse_str(r#"(module (memory (export "memory") 1) (func (export "mixless_buffer") (result i32) (i32.const 0)) (func (export "mixless_event") (param i32 i32 i32 i32 i64) (result i32) (i32.const 0)))"#).unwrap());
    let file = write(dir.path(), "adapter.json", &adapter);
    let mut bad = hub.preview_import(&file).unwrap();
    bad.preset.adapter.as_mut().unwrap().wasm_base64 = "invalid".into();
    assert!(hub.accept_imports(vec![report, bad]).is_err());
    assert_eq!(hub.presets(), before);
    assert!(!hub.storage_path().exists());
}

#[test]
fn failed_batch_write_keeps_memory_disk_and_runtime_revision_then_retry_saves_all() {
    let dir = tempfile::tempdir().unwrap();
    let hub = manager(dir.path());
    let p = builtin_presets().remove(0);
    let path = write(dir.path(), "a.json", &p);
    let report = hub.preview_import(&path).unwrap();
    hub.accept_import(report.clone()).unwrap();
    let before = hub.presets();
    let file = hub.storage_path();
    let bytes = fs::read(&file).unwrap();
    let revision = hub.shared.revision.load(Ordering::Acquire);
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    assert!(
        hub.accept_imports(vec![report.clone(), report.clone()])
            .is_err()
    );
    assert_eq!(hub.presets(), before);
    assert_eq!(hub.shared.revision.load(Ordering::Acquire), revision);
    fs::remove_dir(&file).unwrap();
    fs::write(&file, &bytes).unwrap();
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert_eq!(
        hub.accept_imports(vec![report.clone(), report])
            .unwrap()
            .len(),
        2
    );
    assert_eq!(hub.presets().len(), before.len() + 2);
}

#[test]
fn empty_and_oversized_batch_requests_do_not_read_or_save_presets() {
    let dir = tempfile::tempdir().unwrap();
    let hub = manager(dir.path());
    assert!(hub.preview_imports(&[]).is_err());
    assert!(hub.accept_imports(vec![]).is_err());
    let path = dir.path().join("large.json");
    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_IMPORT_BYTES + 1).unwrap();
    assert!(
        hub.preview_imports(&[path.clone()])
            .unwrap_err()
            .to_string()
            .contains("64 MiB")
    );
    assert!(
        hub.preview_imports(&vec![path; MAX_IMPORT_FILES + 1])
            .is_err()
    );
    assert!(!hub.storage_path().exists());
}
