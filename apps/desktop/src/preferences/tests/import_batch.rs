use super::*;
use mixless_controllers::{ControllerPreset, ImportBatch};

fn batch_fixture(
    view: &gpui::Entity<Preferences>,
    cx: &mut VisualTestContext,
    temp: &tempfile::TempDir,
) -> ImportBatch {
    view.read_with(cx, |s, _| {
        let h=s.hub().unwrap();
        let a=temp.path().join("ddj.json");
        h.export_preset("builtin-ddj-400-v1",&a).unwrap();
        let mut adapter:ControllerPreset=serde_json::from_str(include_str!("../../../../../crates/mixless-controllers/examples/synthetic-hid/preset.template.json")).unwrap();
        adapter.adapter.as_mut().unwrap().set_module(&wat::parse_str(r#"(module (memory (export "memory") 1) (func (export "mixless_buffer") (result i32) (i32.const 0)) (func (export "mixless_event") (param i32 i32 i32 i32 i64) (result i32) (i32.const 0)))"#).unwrap());
        let b=temp.path().join("hid.json");std::fs::write(&b,serde_json::to_vec(&adapter).unwrap()).unwrap();
        let bad=temp.path().join("invalid.json");std::fs::write(&bad,"{broken").unwrap();
        h.preview_imports(&[a,b,bad]).unwrap()
    })
}
fn deliver(view: &gpui::Entity<Preferences>, cx: &mut VisualTestContext, batch: ImportBatch) {
    view.update(cx, |s, cx| {
        let (tx, rx) = mpsc::channel();
        tx.send(JobResult::PresetImports(Ok(batch))).unwrap();
        s.job = Some(rx);
        s.poll();
        cx.notify();
    });
    cx.run_until_parked();
}
fn finish(view: &gpui::Entity<Preferences>, cx: &mut VisualTestContext) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while view.read_with(cx, |s, _| s.job.is_some()) && std::time::Instant::now() < deadline {
        view.update(cx, |s, cx| {
            s.poll();
            cx.notify();
        });
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(view.read_with(cx, |s, _| s.job.is_none()));
    cx.run_until_parked();
}

#[gpui::test]
fn batch_review_exposes_each_result_and_adapter_details_without_importing(cx: &mut TestAppContext) {
    let (temp, view, cx) = fixture(cx);
    let batch = batch_fixture(&view, cx, &temp);
    let count = view.read_with(cx, |s, _| s.hub().unwrap().presets().len());
    deliver(&view, cx, batch);
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let panel = cx.debug_bounds("controller-dialog-panel").unwrap();
        let footer = cx.debug_bounds("preset-import-footer").unwrap();
        let body = cx.debug_bounds("preset-batch-scroll").unwrap();
        assert!(panel.left() >= px(24.) && panel.right() <= px(width - 24.));
        // Centered odd-height panels can differ by half a logical pixel after
        // native layout rounding; the body must stay above the pinned footer.
        assert!(
            panel.bottom() <= px(height - 24.) && body.bottom() <= footer.top() + px(1.),
            "{width}x{height}: panel {panel:?}, body {body:?}, footer {footer:?}"
        );
        assert!(cx.debug_bounds("import-file-2").is_some());
        assert!(cx.debug_bounds("review-import-2").is_none());
        click(cx, "review-import-1");
        assert!(cx.debug_bounds("import-adapter-access").is_some());
        assert!(cx.debug_bounds("import-community").is_some());
        view.read_with(cx, |s, _| {
            assert!(
                s.import_batch.is_some() && s.import_preview.is_some(),
                "{width}x{height}: Details must preserve the batch"
            )
        });
        // GPUI retains removed debug selectors across recycled frames. Check the
        // review state and library instead of treating an old button bound as live.
        view.read_with(cx, |s, _| {
            assert_eq!(s.hub().unwrap().presets().len(), count)
        });
        click(cx, "cancel-preset-import"); // Back to the complete batch.
        view.read_with(cx, |s, _| {
            assert!(s.import_batch.is_some() && s.import_preview.is_none())
        });
        click(cx, "tab-Audio I/O");
        click(cx, "close-preferences");
        view.read_with(cx, |s, _| {
            assert!(s.tab == Tab::Midi && s.import_batch.is_some())
        });
    }
    click(cx, "review-import-0");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert!(s.import_batch.is_some() && s.import_preview.is_none())
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert!(s.import_batch.is_none());
        assert_eq!(s.hub().unwrap().presets().len(), count);
    });
}

#[gpui::test]
fn batch_import_write_failure_is_retryable_and_success_saves_only_valid_files(
    cx: &mut TestAppContext,
) {
    let (temp, view, cx) = fixture(cx);
    let batch = batch_fixture(&view, cx, &temp);
    let (file, bytes, count, assignments, bindings) = view.read_with(cx, |s, _| {
        let h = s.hub().unwrap();
        h.assign(mixless_controllers::DeviceAssignment {
            device_id: "offline-unit".into(),
            preset_id: "builtin-ddj-400-v1".into(),
            ports: [("main".into(), "offline-unit".into())].into(),
            outputs: Default::default(),
        })
        .unwrap();
        let file = h.storage_path().to_path_buf();
        let bytes = std::fs::read(&file).unwrap();
        (
            file,
            bytes,
            h.presets().len(),
            h.assignments(),
            h.bindings(),
        )
    });
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    deliver(&view, cx, batch);
    click(cx, "accept-preset-import");
    // A saving batch cannot be cancelled while its transaction is running.
    view.read_with(cx, |s, _| assert!(s.job.is_some()));
    cx.simulate_keystrokes("escape");
    finish(&view, cx);
    view.read_with(cx, |s, _| {
        assert!(s.error && s.import_batch.is_some());
        assert_eq!(s.hub().unwrap().presets().len(), count);
    });
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let footer = cx.debug_bounds("preset-import-footer").unwrap();
        let accept = cx.debug_bounds("accept-preset-import").unwrap();
        assert!(footer.bottom() <= px(height - 24.) && accept.bottom() <= footer.bottom());
    }
    std::fs::remove_dir(&file).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    click(cx, "accept-preset-import");
    finish(&view, cx);
    view.read_with(cx, |s, _| {
        assert!(!s.error && s.import_batch.is_none());
        assert!(s.status.contains("Imported 2 presets; skipped 1 file"));
        let h = s.hub().unwrap();
        assert_eq!(h.presets().len(), count + 2);
        assert_eq!(h.assignments(), assignments);
        assert_eq!(h.bindings(), bindings);
        assert!(!s.selected_profile().unwrap().is_builtin());
    });
    let reopened = mixless_controllers::ControllerManager::start_with_config(
        temp.path().join("midi.json"),
        MidiConfig {
            enabled: false,
            input_ids: None,
        },
    )
    .unwrap();
    assert_eq!(reopened.presets().len(), count + 2);
    assert_eq!(reopened.assignments(), assignments);
}

#[gpui::test]
fn long_batch_scroll_keeps_footer_fixed_and_all_invalid_batches_cannot_import(
    cx: &mut TestAppContext,
) {
    let (temp, view, cx) = fixture(cx);
    let batch = batch_fixture(&view, cx, &temp);
    let mut entry = batch.entries[0].clone();
    entry.file_name = "Long controller file name ".repeat(20);
    let large = ImportBatch {
        entries: vec![entry; 40],
    };
    deliver(&view, cx, large);
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let footer = cx.debug_bounds("preset-import-footer").unwrap();
        let body = cx.debug_bounds("preset-batch-scroll").unwrap();
        scroll(cx, body.center(), -10000.);
        let last = cx.debug_bounds("import-file-39").unwrap();
        assert!(last.top() >= body.top() && last.bottom() <= body.bottom());
        assert_eq!(cx.debug_bounds("preset-import-footer").unwrap(), footer);
    }
    deliver(
        &view,
        cx,
        ImportBatch {
            entries: vec![batch.entries[2].clone(); 2],
        },
    );
    let count = view.read_with(cx, |s, _| s.hub().unwrap().presets().len());
    click(cx, "accept-preset-import");
    view.read_with(cx, |s, _| {
        assert!(s.job.is_none());
        assert_eq!(s.import_batch.as_ref().unwrap().ready_count(), 0);
        assert_eq!(s.hub().unwrap().presets().len(), count);
    });
    click(cx, "cancel-preset-import");
    view.read_with(cx, |s, _| assert!(s.import_batch.is_none()));
}
