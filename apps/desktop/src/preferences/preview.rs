//! Development-only native rendering harness. It uses the production view and
//! requires an explicitly isolated data directory. Connected scenes use a real
//! CoreMIDI virtual source, named explicitly by the capture runner.
use super::*;

pub fn run() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--settings-preview") {
        return false;
    }
    let scene = args.get(2).cloned().unwrap_or_else(|| "general".into());
    assert!(
        matches!(
            scene.as_str(),
            "general"
                | "audio"
                | "audio-options"
                | "audio-details"
                | "controllers"
                | "preset-options"
                | "preset-actions"
                | "preset-import-batch"
                | "controller-layout"
                | "controller-details"
                | "controller-calibration"
                | "controller-connected"
                | "controller-connected-details"
                | "adapter-import"
                | "adapter-details"
                | "adapter-mapping"
                | "storage"
                | "storage-loading"
        ),
        "Unknown settings scene"
    );
    let data = std::env::var_os("MIXLESS_DATA_DIR")
        .map(PathBuf::from)
        .expect("Set MIXLESS_DATA_DIR to an isolated preview directory");
    assert!(
        data.is_absolute() && data.starts_with(std::env::temp_dir()),
        "Preview data must be inside the system temporary directory"
    );
    assert!(
        std::env::var("MIXLESS_OFFLINE").as_deref() == Ok("1")
            && std::env::var("MIXLESS_MODELS_OFFLINE").as_deref() == Ok("1"),
        "Preview requires offline mode"
    );
    let width = args
        .get(3)
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(940.)
        .max(720.);
    let height = args
        .get(4)
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(820.)
        .max(540.);
    gpui::Application::new().run(move |cx| {
        let core = crate::state::app_core();
        let preview_device = if scene.starts_with("controller-connected") {
            let name = std::env::var("MIXLESS_PREVIEW_MIDI_INPUT")
                .expect("Connected preview requires the exact virtual MIDI source name");
            let hub = core
                .midi
                .lock()
                .expect("controllers")
                .clone()
                .expect("controller manager");
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let device = loop {
                if let Some(device) = hub.devices().into_iter().find(|d| {
                    d.name == name
                        && !d.hid
                        && d.state == mixless_controllers::ConnectionState::Connected
                }) {
                    break device;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "Preview MIDI source did not connect"
                );
                std::thread::sleep(Duration::from_millis(25));
            };
            let preset = hub.presets().remove(0);
            hub.assign(mixless_controllers::DeviceAssignment {
                device_id: device.id.clone(),
                preset_id: preset.id,
                ports: [("main".into(), device.id.clone())].into(),
                outputs: Default::default(),
            })
            .expect("assign isolated preview preset");
            Some(device)
        } else {
            None
        };
        let devices = mixless_engine::output_devices().unwrap_or_default();
        let storage =
            (scene == "storage").then(|| storage::report(&core).expect("preview storage report"));
        let shutdown = core.clone();
        cx.on_app_quit(move |_| {
            shutdown
                .shutting_down
                .store(true, std::sync::atomic::Ordering::Release);
            async {}
        })
        .detach();
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(width), px(height)),
                        cx,
                    ))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("mixless Preferences".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(gpui::point(
                            px(16.),
                            px((TITLEBAR_HEIGHT - 14.) / 2.),
                        )),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(720.), px(540.))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        let mut state = Preferences::new(core, cx);
                        // Freeze scan states for reproducible native rendering.
                        state.job = None;
                        state.devices = devices;
                        state.storage = storage;
                        if scene == "storage-loading" {
                            state.storage_scan = storage_page::ScanState::Running;
                        }
                        state.tab = match scene.as_str() {
                            "general" => Tab::General,
                            "audio" | "audio-options" | "audio-details" => Tab::Audio,
                            "storage" | "storage-loading" => Tab::Storage,
                            _ => Tab::Midi,
                        };
                        if state.tab == Tab::Midi {
                            state.selected_preset = state
                                .hub()
                                .ok()
                                .and_then(|h| h.presets().first().map(|p| p.id.clone()));
                            if let Some(device) = preview_device {
                                state.controller_device = Some(device.id.clone());
                                state.controller_devices = vec![device];
                                state.bindings = state.hub().expect("controllers").bindings();
                            }
                            state.select_midi_target(MidiTarget::Play { deck: DeckId::A });
                        }
                        state.controller_advanced =
                            scene.ends_with("details") && state.tab == Tab::Midi;
                        if scene.starts_with("adapter-") {
                            let path = std::env::var_os("MIXLESS_PREVIEW_PRESET")
                                .map(PathBuf::from)
                                .expect("Set MIXLESS_PREVIEW_PRESET for an adapter scene");
                            let hub = state.hub().expect("preview controllers");
                            let report = hub.preview_import(&path).expect("valid preview package");
                            if scene == "adapter-import" {
                                state.import_preview = Some(report);
                            } else {
                                state.selected_preset = Some(
                                    hub.accept_import(report).expect("import preview package"),
                                );
                                state.select_midi_target(MidiTarget::Play { deck: DeckId::A });
                            }
                        }
                        if scene == "preset-import-batch" {
                            let paths: Vec<PathBuf> = serde_json::from_str(
                                &std::env::var("MIXLESS_PREVIEW_PRESETS")
                                    .expect("Set MIXLESS_PREVIEW_PRESETS for batch import review"),
                            )
                            .expect("JSON array of preset paths");
                            state.import_batch = Some(
                                state
                                    .hub()
                                    .expect("preview controllers")
                                    .preview_imports(&paths)
                                    .expect("preview batch"),
                            );
                        }
                        if scene == "controller-calibration" {
                            let hub = state.hub().expect("offline preview controllers");
                            hub.upsert(MidiBinding {
                                device_id: None,
                                address: Some(mixless_controllers::MidiAddress::Cc14 {
                                    channel: 0,
                                    msb: 1,
                                    lsb: 33,
                                }),
                                inverted: false,
                                sensitivity: 1.,
                                ticks_per_revolution: None,
                                pickup: false,
                                kind: MidiSourceKind::Cc,
                                channel: 0,
                                number: 1,
                                target: MidiTarget::CueMix,
                                mode: MidiControlMode::Absolute,
                            })
                            .expect("save synthetic preview source");
                            state.controller_device = Some("offline-calibration-fixture".into());
                            state.midi.enabled = true;
                            state.midi.input_ids = Some(vec!["offline-calibration-fixture".into()]);
                            state.selected_preset = None;
                            state.bindings = hub.bindings();
                            state.select_midi_target(MidiTarget::CueMix);
                            state.mapping_calibration_open = true;
                        }
                        state.audio_details_open = scene == "audio-details";
                        state
                    })
                },
            )
            .expect("open settings preview");
        // Measure the production controls before seeding an open menu. The
        // locked desktop may suspend display-link frames, so the complete scene
        // must be ready for the window's first native presentation.
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })
        .expect("measure preview");
        window
            .update(cx, |state, window, cx| {
                match scene.as_str() {
                    "audio-options" => state.toggle_selector(Selector::Buffer, window),
                    "preset-options" => state.toggle_selector(Selector::MidiPreset, window),
                    "preset-actions" => state.toggle_selector(Selector::PresetActions, window),
                    _ => {}
                }
                cx.notify();
            })
            .expect("prepare preview scene");
        cx.activate(true);
    });
    true
}
