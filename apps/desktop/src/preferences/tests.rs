//! Interaction/layout regressions using the actual Preferences render tree.
use super::*;
use gpui::{TestAppContext, VisualTestContext};
mod controller_audio;
mod general;
mod import_batch;

fn fixture(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    gpui::Entity<Preferences>,
    &mut VisualTestContext,
) {
    let (temp, mut core, _) = crate::automix::tests::fixture();
    Arc::get_mut(&mut core).expect("isolated fixture").stems = Some(mixless_stems::Processor::new(
        temp.path().join("models"),
        temp.path().join("stem-cache"),
        false,
    ));
    let hub = mixless_controllers::ControllerManager::start_with_config(
        temp.path().join("midi.json"),
        MidiConfig {
            enabled: false,
            input_ids: None,
        },
    )
    .unwrap();
    *core.midi.lock().unwrap() = Some(Arc::new(hub));
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut state = Preferences::new(core, cx);
        state.tab = Tab::Midi;
        state.job = None;
        state
    });
    cx.simulate_resize(size(px(940.), px(820.)));
    cx.run_until_parked();
    (temp, view, cx)
}
fn click(cx: &mut VisualTestContext, id: &'static str) {
    let bounds = cx
        .debug_bounds(id)
        .unwrap_or_else(|| panic!("missing {id}"));
    cx.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
}

#[gpui::test]
#[cfg(target_os = "macos")]
#[ignore = "creates one explicitly selected CoreMIDI virtual source; no physical inputs"]
fn quick_learn_preserves_an_existing_cc14_pair_through_virtual_input(cx: &mut TestAppContext) {
    use mixless_midi::midir::{MidiOutput, os::unix::VirtualOutput};
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let (temp, view, cx) = fixture(cx);
    let name = format!(
        "mixless-review-{}",
        temp.path().file_name().unwrap().to_string_lossy()
    );
    let mut source = MidiOutput::new("mixless-review")
        .unwrap()
        .create_virtual(&name)
        .unwrap();
    let hub = view.read_with(cx, |s, _| s.hub().unwrap());
    let wait = |condition: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !condition() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(condition());
    };
    wait(&|| hub.devices().iter().any(|d| d.name == name && !d.hid));
    let id = hub
        .devices()
        .into_iter()
        .find(|d| d.name == name && !d.hid)
        .unwrap()
        .id;
    hub.configure(MidiConfig {
        enabled: true,
        input_ids: Some(vec![id.clone()]),
    })
    .unwrap();
    wait(&|| {
        hub.devices()
            .iter()
            .any(|d| d.id == id && d.state == mixless_controllers::ConnectionState::Connected)
    });
    view.update(cx, |s, cx| {
        s.controller_device = Some(id.clone());
        s.binding.target = MidiTarget::CueMix;
        s.binding.device_id = Some(id.clone());
        s.binding
            .set_address(mixless_controllers::MidiAddress::Cc14 {
                channel: 0,
                msb: 1,
                lsb: 33,
            });
        s.binding.sensitivity = 0.5;
        hub.upsert(s.binding.clone()).unwrap();
        s.poll();
        s.select_midi_target(MidiTarget::CueMix);
        s.controller_devices = hub.devices();
        s.midi = hub.config();
        cx.notify();
    });
    cx.run_until_parked();
    click(cx, "diagram-CueMix");
    view.read_with(cx, |s, _| assert!(s.learning && s.quick_learning));
    source.send(&[0xb0, 1, 100]).unwrap();
    wait(&|| hub.last_input().is_some_and(|input| input.value == 100));
    assert!(
        hub.learned().is_none(),
        "Shortcut Learn accepted only the MSB"
    );
    source.send(&[0xb0, 33, 7]).unwrap();
    wait(&|| hub.learned().is_some());
    assert_eq!(hub.learned().unwrap().value, 12807);
    view.update(cx, |s, _| {
        s.poll();
        assert!(!s.learning && !s.quick_learning);
    });
    let binding = hub
        .bindings()
        .into_iter()
        .find(|b| b.target == MidiTarget::CueMix)
        .unwrap();
    assert_eq!(
        binding.midi_address(),
        mixless_controllers::MidiAddress::Cc14 {
            channel: 0,
            msb: 1,
            lsb: 33
        }
    );
    assert_eq!(binding.sensitivity, 0.5);
    hub.configure(MidiConfig {
        enabled: false,
        input_ids: Some(vec![]),
    })
    .unwrap();
    drop(source);
}

#[gpui::test]
fn editing_a_midi_address_disarms_pending_learning(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        for (selector, value) in [
            (Selector::MidiType, "cc14"),
            (Selector::MidiChannel, "2"),
            (Selector::MidiNumber, "4"),
            (Selector::MidiLsb, "36"),
        ] {
            s.learning = true;
            s.quick_learning = true;
            s.choose(selector, value, cx);
            assert!(!s.learning && !s.quick_learning);
        }
        assert_eq!(
            s.binding.midi_address(),
            mixless_controllers::MidiAddress::Cc14 {
                channel: 1,
                msb: 4,
                lsb: 36
            }
        );
        assert!(!s.hub().unwrap().config().enabled);
    });
}

#[gpui::test]
fn ni_audio_routing_is_a_reviewable_draft_without_a_control_preset(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        s.controller_devices = vec![mixless_controllers::ControllerDevice {
            id: "offline-ni-audio-fixture".into(),
            name: "Traktor Kontrol S4 MK3".into(),
            hid: false,
            state: mixless_controllers::ConnectionState::Connected,
            vendor_id: None,
            product_id: None,
            interface: None,
            usage_page: None,
            usage: None,
        }];
        s.controller_device = Some("offline-ni-audio-fixture".into());
        s.selected_preset = None;
        s.controller_advanced = true;
        s.core
            .settings
            .update(|settings| {
                settings.audio.master_device = Some("Stored output".into());
                settings.audio.cue_device = None;
            })
            .unwrap();
        assert!(s.selected_profile().is_none());
        assert!(!s.hub().unwrap().config().enabled);
        cx.notify();
    });
    cx.run_until_parked();
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let button = cx.debug_bounds("controller-audio").unwrap();
        assert!(button.left() >= px(0.) && button.right() <= px(width));
    }
    let button = cx.debug_bounds("controller-audio").unwrap();
    if button.center().y > px(420.) {
        let content = cx.debug_bounds("settings-content").unwrap();
        scroll(cx, content.center(), 380. - f32::from(button.center().y));
    }
    click(cx, "controller-audio");
    view.read_with(cx, |s, _| {
        assert!(s.tab == Tab::Audio);
        assert_eq!(
            s.audio.master_device.as_deref(),
            Some("Traktor Kontrol S4 MK3")
        );
        assert_eq!(s.audio.cue_device, s.audio.master_device);
        assert_eq!(
            (s.audio.master_channels, s.audio.cue_channels),
            ([0, 1], [2, 3])
        );
        assert_eq!(
            s.core.settings.get().audio.master_device.as_deref(),
            Some("Stored output")
        );
        assert!(s.core.settings.get().audio.cue_device.is_none());
        assert!(s.hub().unwrap().assignments().is_empty());
        assert!(!s.hub().unwrap().config().enabled);
    });
}

#[gpui::test]
fn unknown_ni_generation_has_no_audio_routing_proposal(cx: &mut TestAppContext) {
    // GPUI 0.2.2 retains old debug bounds across frames. Check absence in a
    // fresh window that has never rendered the recommendation button.
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        s.controller_devices = vec![mixless_controllers::ControllerDevice {
            id: "offline-unknown-ni-fixture".into(),
            name: "Traktor Kontrol S4 MK4".into(),
            hid: false,
            state: mixless_controllers::ConnectionState::Connected,
            vendor_id: None,
            product_id: None,
            interface: None,
            usage_page: None,
            usage: None,
        }];
        s.controller_device = Some("offline-unknown-ni-fixture".into());
        s.selected_preset = None;
        s.controller_advanced = true;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("controller-audio").is_none());
}

#[gpui::test]
fn compound_source_editor_and_live_headphone_mix_preserve_parameters(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        // UI-only identity; the hub remains disabled and no input is opened.
        s.controller_device = Some("offline-ui-fixture".into());
        s.mapping_calibration_open = true;
        s.select_midi_target(MidiTarget::CueMix);
        s.choose(Selector::MidiType, "cc14", cx);
        s.choose(Selector::MidiChannel, "4", cx);
        s.choose(Selector::MidiNumber, "5", cx);
        s.choose(Selector::MidiLsb, "37", cx);
        s.choose(Selector::MidiSensitivity, "2", cx);
        s.choose(Selector::MidiTicks, "1024", cx);
        assert_eq!(
            s.binding.midi_address(),
            mixless_controllers::MidiAddress::Cc14 {
                channel: 3,
                msb: 5,
                lsb: 37
            }
        );
        assert_eq!(s.binding.sensitivity, 2.);
        assert_eq!(s.binding.ticks_per_revolution, Some(1024.));
        assert!(
            s.selector_options(Selector::MidiType)
                .iter()
                .any(|(id, _)| id == "pitch_bend")
        );
        s.choose(Selector::MidiType, "pitch_bend", cx);
        assert_eq!(
            s.binding.midi_address(),
            mixless_controllers::MidiAddress::PitchBend { channel: 3 }
        );
        s.core
            .engine
            .dispatch(Command::SetCueMix { value: 0.05 })
            .unwrap();
        // The stored default is already zero. Applying zero still changes a MIDI-updated engine.
        s.save_headphone_mix(0.);
        assert_eq!(s.core.engine.snapshot().cue_mix, 0.);
        assert_eq!(s.core.settings.get().cue_mix, 0.);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("mapping-calibration").is_some());
    assert!(cx.debug_bounds("select-14").is_some());
    view.update(cx, |s, cx| {
        s.choose(Selector::MidiType, "cc14", cx);
        s.choose(Selector::MidiNumber, "5", cx);
        s.choose(Selector::MidiLsb, "5", cx);
        s.save_binding();
        assert!(s.error && s.status.contains("MIDI address"));
        cx.notify();
    });
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        // Selector debug IDs are stable numeric tags, as in the other menu tests.
        for id in ["select-14", "select-9", "select-15"] {
            let bounds = cx.debug_bounds(id).unwrap();
            assert!(bounds.left() >= px(0.) && bounds.right() <= px(width));
        }
        let done = cx.debug_bounds("close-preferences").unwrap();
        assert!(done.bottom() <= px(height));
    }
}

#[gpui::test]
fn community_adapter_import_is_explicit_and_mapping_inspector_is_read_only(
    cx: &mut TestAppContext,
) {
    let (temp, view, cx) = fixture(cx);
    let mut preset: mixless_controllers::ControllerPreset = serde_json::from_str(include_str!(
        "../../../../crates/mixless-controllers/examples/synthetic-hid/preset.template.json"
    ))
    .unwrap();
    let module = wat::parse_str(
        r#"(module (memory (export "memory") 1)
        (func (export "mixless_buffer") (result i32) (i32.const 0))
        (func (export "mixless_event") (param i32 i32 i32 i32 i64) (result i32) (i32.const 0)))"#,
    )
    .unwrap();
    preset.adapter.as_mut().unwrap().set_module(&module);
    let path = temp.path().join("community.json");
    std::fs::write(&path, serde_json::to_vec(&preset).unwrap()).unwrap();
    let report = view.read_with(cx, |s, _| s.hub().unwrap().preview_import(&path).unwrap());
    // A large permission manifest must scroll independently of confirmation,
    // including when the save fails. Long metadata stays in its own columns.
    let mut large = report.clone();
    large.preset.name = "Long community controller preset name ".repeat(5);
    large.preset.community.as_mut().unwrap().author = "Community controller author ".repeat(7);
    let permission = large.preset.adapter.as_ref().unwrap().reports[0].clone();
    large.preset.adapter.as_mut().unwrap().reports = (0..128)
        .map(
            |report_id| mixless_controllers::community::ReportPermission {
                report_id,
                ..permission.clone()
            },
        )
        .collect();
    large.conflicts = vec!["A preset with this name already exists".into()];
    view.update(cx, |s, cx| {
        s.import_preview = Some(Arc::new(large));
        s.error = true;
        s.status = "Unable to save the preset. Check folder permissions. ".repeat(8);
        cx.notify();
    });
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let panel = cx.debug_bounds("controller-dialog-panel").unwrap();
        let footer = cx.debug_bounds("preset-import-footer").unwrap();
        let accept = cx.debug_bounds("accept-preset-import").unwrap();
        assert!(panel.left() >= px(24.) && panel.right() <= px(width - 24.));
        assert!(accept.bottom() <= panel.bottom() && panel.bottom() <= px(height - 24.));
        let body = cx.debug_bounds("preset-import-scroll").unwrap();
        assert!(body.bottom() <= footer.top());
        scroll(
            cx,
            gpui::point(body.right() - px(3.), body.center().y),
            -10000.,
        );
        let last = cx.debug_bounds("import-report-127").unwrap();
        assert!(
            last.top() >= body.top() && last.bottom() <= body.bottom(),
            "{last:?} in {body:?}"
        );
        assert_eq!(cx.debug_bounds("preset-import-footer").unwrap(), footer);
        assert_eq!(cx.debug_bounds("accept-preset-import").unwrap(), accept);
    }
    view.update(cx, |s, cx| {
        s.import_preview = Some(Arc::new(report.clone()));
        s.error = false;
        s.status.clear();
        cx.notify();
    });
    cx.simulate_resize(size(px(720.), px(540.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("import-community").is_some());
    assert!(cx.debug_bounds("import-adapter-access").is_some());
    let accept = cx.debug_bounds("accept-preset-import").unwrap();
    assert!(accept.bottom() <= px(540.));
    click(cx, "cancel-preset-import");
    view.read_with(cx, |s, _| {
        assert!(
            !s.hub()
                .unwrap()
                .presets()
                .iter()
                .any(|p| p.community.is_some())
        )
    });
    view.update(cx, |s, cx| {
        s.import_preview = Some(Arc::new(report));
        cx.notify();
    });
    cx.run_until_parked();
    click(cx, "accept-preset-import");
    view.update(cx, |s, cx| {
        s.select_midi_target(MidiTarget::Play { deck: DeckId::A });
        cx.notify();
    });
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert!(s.hub().unwrap().assignments().is_empty());
        assert_eq!(
            s.selected_profile().unwrap().verification,
            mixless_controllers::Verification::Protocol
        );
        assert!(!s.preset_actions().iter().any(|(v, _)| v == "edit"));
    });
    assert!(cx.debug_bounds("mapping-preview").is_some());
    assert!(cx.debug_bounds("edit-preset-mapping").is_none());
}

fn preset_action(view: &gpui::Entity<Preferences>, cx: &mut VisualTestContext, value: &str) {
    click(cx, "select-13");
    let index = view.read_with(cx, |s, _| {
        s.preset_actions()
            .iter()
            .position(|(v, _)| v == value)
            .unwrap()
    });
    click(
        cx,
        [
            "option-0", "option-1", "option-2", "option-3", "option-4", "option-5", "option-6",
        ][index],
    );
}

#[gpui::test]
fn controller_diagram_preserves_deck_geometry_when_resized(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    for (width, height) in [(940., 820.), (720., 540.), (1200., 900.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let diagram = cx.debug_bounds("controller-diagram").unwrap();
        let a = cx.debug_bounds("diagram-Play { deck: A }").unwrap();
        let b = cx.debug_bounds("diagram-Play { deck: B }").unwrap();
        assert!(diagram.size.width >= px(672.), "{diagram:?}");
        assert!(a.size.width > px(20.), "{a:?}");
        assert!(b.left() > a.right() + px(250.), "{a:?} {b:?}");
        assert!(a.left() >= diagram.left() && b.right() <= diagram.right());
        let ring = cx.debug_bounds("diagram-Position { deck: A }").unwrap();
        let jog = cx.debug_bounds("diagram-Jog { deck: A }").unwrap();
        assert!(ring.contains(&jog.center()));
        assert!(ring.size.width > jog.size.width);
        cx.simulate_click(
            gpui::point(ring.center().x, ring.top() + px(4.)),
            Default::default(),
        );
        cx.run_until_parked();
        view.read_with(cx, |s, _| {
            assert_eq!(s.binding.target, MidiTarget::Position { deck: DeckId::A })
        });
        click(cx, "diagram-Jog { deck: A }");
        view.read_with(cx, |s, _| {
            assert_eq!(s.binding.target, MidiTarget::Jog { deck: DeckId::A })
        });
        click(cx, "diagram-TimeDisplay { deck: A }");
        view.read_with(cx, |s, _| {
            assert_eq!(
                s.binding.target,
                MidiTarget::TimeDisplay { deck: DeckId::A }
            )
        });
    }
}

#[gpui::test]
fn preset_selection_is_offline_and_modal_blocks_background_clicks(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    let builtin_count = mixless_controllers::builtin_presets().len();
    // Export is unavailable until a preset is chosen.
    view.read_with(cx, |s, _| {
        assert!(!s.preset_actions().iter().any(|(v, _)| v == "export"))
    });
    view.read_with(cx, |s, _| assert!(s.job.is_none() && !s.error));
    click(cx, "select-11");
    view.read_with(cx, |s, _| {
        assert_eq!(
            s.selector_options(Selector::MidiPreset).len(),
            builtin_count + 1
        )
    });
    let index = view.read_with(cx, |s, _| {
        s.selector_options(Selector::MidiPreset)
            .iter()
            .position(|(id, _)| id == "builtin-ddj-400-v1")
            .unwrap()
    });
    cx.simulate_keystrokes("home");
    for _ in 0..index {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert_eq!(s.selected_profile().unwrap().name, "DDJ-400");
        assert!(s.hub().unwrap().assignments().is_empty());
        assert!(s.selector.is_none());
    });
    click(cx, "controller-apply");
    view.read_with(cx, |s, _| {
        assert!(s.hub().unwrap().assignments().is_empty())
    });
    cx.simulate_resize(size(px(720.), px(540.)));
    cx.run_until_parked();
    preset_action(&view, cx, "edit");
    let save = cx.debug_bounds("save-preset-edit").unwrap();
    assert!(save.bottom() <= px(540.), "Save clipped: {save:?}");
    // The background tab and Done must not change/close Preferences during editing.
    click(cx, "tab-Audio I/O");
    click(cx, "close-preferences");
    view.read_with(cx, |s, _| {
        assert!(s.tab == Tab::Midi && s.preset_dialog.is_some())
    });
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |s, _| assert!(s.preset_dialog.is_none()));
    preset_action(&view, cx, "new");
    cx.simulate_keystrokes("cmd-a");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("界面测试".into()));
    cx.simulate_keystrokes("cmd-v enter");
    view.read_with(cx, |s, _| {
        assert_eq!(s.selected_profile().unwrap().name, "界面测试");
        assert!(!s.error && s.preset_dialog.is_none());
        assert!(s.hub().unwrap().assignments().is_empty());
    });
}

#[gpui::test]
fn none_preset_clears_preview_and_applies_only_to_the_selected_device(cx: &mut TestAppContext) {
    let (temp, view, cx) = fixture(cx);
    let profile = view.read_with(cx, |s, _| {
        let options = s.selector_options(Selector::MidiPreset);
        assert_eq!(options[0], (String::new(), "None".into()));
        assert!(options.iter().all(|(_, name)| !name.contains('(')));
        let profile = s
            .hub()
            .unwrap()
            .presets()
            .into_iter()
            .find(|p| p.name.starts_with("Traktor Kontrol F1"))
            .unwrap();
        assert_eq!(
            options.iter().find(|(id, _)| id == &profile.id).unwrap().1,
            "Traktor Kontrol F1"
        );
        assert!(profile.notes.contains("FL Studio"));
        profile
    });
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        view.update(cx, |s, cx| {
            s.controller_device = None;
            s.selected_preset = Some(profile.id.clone());
            cx.notify();
        });
        cx.run_until_parked();
        // Keyboard None clears an offline preview without creating assignments.
        click(cx, "select-11");
        cx.simulate_keystrokes("home enter");
        cx.run_until_parked();
        view.read_with(cx, |s, _| {
            assert!(s.selected_profile().is_none());
            assert!(s.hub().unwrap().assignments().is_empty());
        });
        view.update(cx, |s, cx| {
            for device in ["none-preview-device", "other-preview-device"] {
                s.hub()
                    .unwrap()
                    .assign(mixless_controllers::DeviceAssignment {
                        device_id: device.into(),
                        preset_id: profile.id.clone(),
                        ports: [("main".into(), device.into())].into(),
                        outputs: Default::default(),
                    })
                    .unwrap();
            }
            s.controller_devices = vec![mixless_controllers::ControllerDevice {
                id: "none-preview-device".into(),
                name: "Offline UI fixture".into(),
                hid: false,
                state: mixless_controllers::ConnectionState::Offline,
                vendor_id: None,
                product_id: None,
                interface: None,
                usage_page: None,
                usage: None,
            }];
            s.controller_device = Some("none-preview-device".into());
            s.selected_preset = Some(profile.id.clone());
            cx.notify();
        });
        cx.run_until_parked();
        click(cx, "select-11");
        cx.simulate_keystrokes("home");
        cx.run_until_parked();
        click(cx, "option-0");
        view.read_with(cx, |s, _| {
            assert!(s.selected_profile().is_none());
            assert_eq!(
                s.hub().unwrap().assignments().len(),
                2,
                "None must wait for Apply"
            );
        });
        click(cx, "controller-apply");
        view.read_with(cx, |s, _| {
            assert!(!s.error);
            assert_eq!(
                s.hub().unwrap().assignments()[0].device_id,
                "other-preview-device"
            );
        });
        // Reopening the stored assignments must preserve the removal.
        let reload = mixless_controllers::ControllerManager::start_with_config(
            temp.path().join("midi.json"),
            MidiConfig {
                enabled: false,
                input_ids: None,
            },
        )
        .unwrap();
        assert_eq!(reload.assignments().len(), 1);
        assert_eq!(reload.assignments()[0].device_id, "other-preview-device");
        view.update(cx, |s, _| {
            s.hub().unwrap().unassign("other-preview-device").unwrap();
        });
    }
}

#[gpui::test]
fn import_retry_clears_previous_error_and_cancel_keeps_library(cx: &mut TestAppContext) {
    let (temp, view, cx) = fixture(cx);
    let builtin_count = mixless_controllers::builtin_presets().len();
    let path = temp.path().join("portable.json");
    let report = view.read_with(cx, |s, _| {
        let hub = s.hub().unwrap();
        let preset = hub.presets().remove(0);
        hub.export_preset(&preset.id, &path).unwrap();
        hub.preview_import(&path).unwrap()
    });
    let deliver = |view: &gpui::Entity<Preferences>, cx: &mut VisualTestContext, result| {
        view.update(cx, |s, cx| {
            let (tx, rx) = mpsc::channel();
            tx.send(JobResult::PresetImport(result)).unwrap();
            s.job = Some(rx);
            s.poll();
            cx.notify();
        });
        cx.run_until_parked();
    };
    deliver(&view, cx, Err("Unsupported preset version 999".into()));
    view.read_with(cx, |s, _| assert!(s.error && s.import_preview.is_none()));
    deliver(&view, cx, Ok(report.clone()));
    view.read_with(cx, |s, _| assert!(!s.error && s.status.is_empty()));
    click(cx, "cancel-preset-import");
    view.read_with(cx, |s, _| {
        assert!(s.import_preview.is_none());
        assert_eq!(s.hub().unwrap().presets().len(), builtin_count);
    });
    deliver(&view, cx, Ok(report));
    click(cx, "accept-preset-import");
    view.read_with(cx, |s, _| {
        assert!(!s.selected_profile().unwrap().is_builtin());
        assert!(s.hub().unwrap().assignments().is_empty());
        assert_eq!(s.hub().unwrap().presets().len(), builtin_count + 1);
    });
    preset_action(&view, cx, "edit");
    cx.simulate_keystrokes("cmd-a");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("{bad json".into()));
    cx.simulate_keystrokes("cmd-v cmd-enter");
    view.read_with(cx, |s, _| {
        assert!(s.error && s.preset_dialog.is_some());
        assert_eq!(s.selected_profile().unwrap().name, "DDJ-400");
        assert_eq!(s.hub().unwrap().presets().len(), builtin_count + 1);
    });
    cx.simulate_keystrokes("escape");
}

#[gpui::test]
fn device_menu_prioritizes_midi_and_does_not_scroll_page_beneath(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        s.controller_devices = (0..40)
            .map(|n| mixless_controllers::ControllerDevice {
                id: format!("hid-{n}"),
                name: "Other interface".into(),
                hid: true,
                state: mixless_controllers::ConnectionState::Discovered,
                vendor_id: Some(0x1209),
                product_id: Some(1),
                interface: Some(n),
                usage_page: Some(0xff00),
                usage: Some(1),
            })
            .collect();
        s.controller_devices
            .push(mixless_controllers::ControllerDevice {
                id: "midi-test".into(),
                name: "DDJ-400".into(),
                hid: false,
                state: mixless_controllers::ConnectionState::Connected,
                vendor_id: None,
                product_id: None,
                interface: None,
                usage_page: None,
                usage: None,
            });
        assert_eq!(
            s.selector_options(Selector::ControllerDevice)[1].0,
            "midi-test"
        );
        cx.notify();
    });
    cx.run_until_parked();
    click(cx, "select-12");
    let page = cx.debug_bounds("mapping-list").unwrap();
    let menu = cx.debug_bounds("preference-options").unwrap();
    cx.simulate_mouse_move(menu.center(), None, Default::default());
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: menu.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-300.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("mapping-list").unwrap(), page);
    assert!(cx.debug_bounds("option-0").unwrap().top() < menu.top());
}

#[gpui::test]
fn controller_choices_use_canonical_names_natural_order_and_stable_ids(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, _| {
        let presets = s.selector_options(Selector::MidiPreset);
        assert_eq!(presets[0], (String::new(), "None".into()));
        let ddj: Vec<_> = presets
            .iter()
            .filter(|(_, name)| name.starts_with("DDJ-"))
            .map(|(_, name)| name.as_str())
            .collect();
        assert_eq!(
            &ddj[..6],
            &[
                "DDJ-200",
                "DDJ-400",
                "DDJ-800",
                "DDJ-1000",
                "DDJ-1000SRT",
                "DDJ-FLX2"
            ]
        );
        assert!(
            presets[1..]
                .windows(2)
                .all(|pair| controllers::natural_cmp(&pair[0].1, &pair[1].1)
                    != std::cmp::Ordering::Greater)
        );
        let device = |id: &str, name: &str, hid| mixless_controllers::ControllerDevice {
            id: id.into(),
            name: name.into(),
            hid,
            state: mixless_controllers::ConnectionState::Discovered,
            vendor_id: None,
            product_id: None,
            interface: None,
            usage_page: None,
            usage: None,
        };
        s.controller_devices = vec![
            device("large", "ddj-1000", false),
            device("h", "DDJ-200", true),
            device("small", "  DDJ-800  ", false),
            device("unknown", "Custom   DJ 2", false),
            device("duplicate", "DDJ-800", false),
        ];
        let options = s.selector_options(Selector::ControllerDevice);
        assert_eq!(
            options
                .iter()
                .skip(1)
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            ["unknown", "duplicate", "small", "large", "h"]
        );
        assert_eq!(s.controller_name("  ddj-flx6-gt  "), "DDJ-FLX6-GT");
        assert_eq!(s.controller_name("Custom   DJ 2"), "Custom DJ 2");
        assert!(options[2].1.starts_with("DDJ-800"));
        assert_ne!(options[2].1, options[3].1);
        assert_eq!(options[4].1, "DDJ-1000");
    });
}

fn scroll(cx: &mut VisualTestContext, position: gpui::Point<gpui::Pixels>, delta: f32) {
    cx.simulate_mouse_move(position, None, Default::default());
    cx.simulate_event(gpui::ScrollWheelEvent {
        position,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(delta))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
}

#[gpui::test]
fn mapping_list_scroll_is_isolated_at_middle_and_both_edges(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let content = cx.debug_bounds("settings-content").unwrap();
        let list = cx.debug_bounds("mapping-list").unwrap();
        let outside = gpui::point(content.left() + px(2.), content.top() + px(20.));
        scroll(cx, outside, f32::from(content.top() + px(12.) - list.top()));
        let list = cx.debug_bounds("mapping-list").unwrap();
        let inside = list.intersect(&content).center();
        let before = view.read_with(cx, |s, _| s.midi_scroll.0.borrow().base_handle.offset());
        scroll(cx, inside, -90.);
        let after = view.read_with(cx, |s, _| s.midi_scroll.0.borrow().base_handle.offset());
        assert!(after.y < before.y, "inner list did not scroll");
        assert_eq!(
            cx.debug_bounds("mapping-list").unwrap(),
            list,
            "outer page scrolled with the list"
        );
        // Scroll to the end and keep scrolling: no hand-off to the page.
        for delta in [-10000., -90., 10000., 90.] {
            scroll(cx, inside, delta);
            assert_eq!(
                cx.debug_bounds("mapping-list").unwrap(),
                list,
                "edge leaked scroll into the page"
            );
        }
        // The page still responds when the pointer leaves the list.
        scroll(cx, outside, 90.);
        assert_ne!(
            cx.debug_bounds("mapping-list").unwrap(),
            list,
            "outer page stopped scrolling"
        );
    }
}

#[gpui::test]
fn selects_anchor_to_controls_toggle_switch_and_dismiss_without_clickthrough(
    cx: &mut TestAppContext,
) {
    let (_temp, view, cx) = fixture(cx);
    cx.simulate_resize(size(px(720.), px(540.)));
    click(cx, "tab-Audio I/O");
    click(cx, "select-0");
    let trigger = cx.debug_bounds("select-0").unwrap();
    let menu = cx.debug_bounds("preference-options").unwrap();
    assert_eq!(menu.left(), trigger.left());
    assert!(menu.top() >= trigger.bottom() && menu.top() - trigger.bottom() <= px(8.));
    assert!(menu.right() <= px(708.) && menu.bottom() <= px(528.));
    // Leaving the trigger/menu does not cause flicker or dismissal.
    cx.simulate_mouse_move(gpui::point(px(30.), px(110.)), None, Default::default());
    cx.run_until_parked();
    view.read_with(cx, |s, _| assert_eq!(s.selector, Some(Selector::Master)));
    // Clicking the same trigger closes it; another trigger switches in one click.
    click(cx, "select-0");
    view.read_with(cx, |s, _| assert!(s.selector.is_none()));
    click(cx, "select-0");
    click(cx, "select-2");
    view.read_with(cx, |s, _| {
        assert_eq!(s.selector, Some(Selector::MasterChannels))
    });
    // Outside clicks close only the menu, never activate a background tab/button.
    click(cx, "tab-General");
    view.read_with(cx, |s, _| {
        assert!(s.selector.is_none() && s.tab == Tab::Audio)
    });
    click(cx, "select-0");
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |s, _| assert!(s.selector.is_none()));
    click(cx, "select-0");
    cx.simulate_resize(size(px(940.), px(820.)));
    cx.run_until_parked();
    view.read_with(cx, |s, _| assert!(s.selector.is_none()));
    // Headphone channels are unavailable until a headphone output is enabled.
    click(cx, "select-3");
    view.read_with(cx, |s, _| assert!(s.selector.is_none()));
    // A scrolled-out control behind the fixed tabs must never intercept dismissal.
    let master = cx.debug_bounds("select-0").unwrap();
    let content = cx.debug_bounds("settings-content").unwrap();
    scroll(cx, content.center(), 65. - f32::from(master.top()));
    click(cx, "select-1");
    view.read_with(cx, |s, _| assert_eq!(s.selector, Some(Selector::Cue)));
    cx.simulate_click(gpui::point(master.center().x, px(75.)), Default::default());
    cx.run_until_parked();
    view.read_with(cx, |s, _| assert!(s.selector.is_none()));
}

#[gpui::test]
fn long_selects_scroll_to_current_value_and_flip_above_low_controls(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        s.controller_device = Some("synthetic-midi-input".into());
        s.binding.number = 126;
        cx.notify();
    });
    cx.simulate_resize(size(px(720.), px(540.)));
    cx.run_until_parked();
    let content = cx.debug_bounds("settings-content").unwrap();
    let number = cx.debug_bounds("select-9").unwrap();
    scroll(
        cx,
        gpui::point(content.right() - px(4.), px(400.)),
        420. - f32::from(number.top()),
    );
    click(cx, "select-9");
    let trigger = cx.debug_bounds("select-9").unwrap();
    let menu = cx.debug_bounds("preference-options").unwrap();
    assert!(menu.bottom() <= trigger.top(), "{menu:?} {trigger:?}");
    assert!(menu.top() >= px(58.) && menu.left() >= px(12.) && menu.right() <= px(708.));
    let selected = cx.debug_bounds("option-126").unwrap();
    assert!(
        selected.top() >= menu.top() && selected.bottom() <= menu.bottom(),
        "selected {selected:?} menu {menu:?}"
    );
    cx.simulate_keystrokes("up enter");
    view.read_with(cx, |s, _| {
        assert!(s.binding.number == 125 && s.selector.is_none())
    });
    click(cx, "select-9");
    let before = cx.debug_bounds("select-9").unwrap();
    scroll(cx, gpui::point(px(30.), px(250.)), 200.);
    view.read_with(cx, |s, _| assert!(s.selector.is_none()));
    assert_eq!(
        cx.debug_bounds("select-9").unwrap(),
        before,
        "closing scroll leaked through"
    );
    // Once dismissed, scrolling the page works again.
    scroll(cx, gpui::point(px(30.), px(250.)), 200.);
    assert_ne!(cx.debug_bounds("select-9").unwrap(), before);
}

#[gpui::test]
fn preset_text_supports_mouse_caret_and_reset_requires_confirmation(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    preset_action(&view, cx, "new");
    let field = cx.debug_bounds("preset-text").unwrap();
    cx.simulate_click(
        gpui::point(field.left() + px(13.), field.top() + px(20.)),
        Default::default(),
    );
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("Studio ".into()));
    cx.simulate_keystrokes("cmd-v enter");
    view.read_with(cx, |s, _| {
        assert_eq!(s.selected_profile().unwrap().name, "Studio My controller")
    });
    preset_action(&view, cx, "rename");
    let field = cx.debug_bounds("preset-text").unwrap();
    let left = gpui::point(field.left() + px(13.), field.top() + px(20.));
    let right = gpui::point(field.right() - px(13.), field.top() + px(20.));
    cx.simulate_mouse_down(left, gpui::MouseButton::Left, Default::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(right, Some(gpui::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(right, gpui::MouseButton::Left, Default::default());
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("Desk".into()));
    cx.simulate_keystrokes("cmd-v enter");
    view.read_with(cx, |s, _| {
        assert_eq!(s.selected_profile().unwrap().name, "Desk")
    });
    click(cx, "tab-General");
    let before = view.read_with(cx, |s, _| s.core.settings.get().show_fx);
    click(cx, "show-fx");
    view.read_with(cx, |s, _| {
        assert_eq!(s.core.settings.get().show_fx, !before)
    });
    click(cx, "reset-defaults");
    click(cx, "show-fx");
    view.read_with(cx, |s, _| {
        assert!(s.confirm_reset && s.core.settings.get().show_fx == !before)
    });
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |s, _| {
        assert!(!s.confirm_reset && s.core.settings.get().show_fx == !before)
    });
    click(cx, "reset-defaults");
    click(cx, "cancel-reset");
    view.read_with(cx, |s, _| {
        assert!(!s.confirm_reset && s.core.settings.get().show_fx == !before)
    });
}

#[gpui::test]
fn storage_loading_waits_for_the_initial_device_scan_then_finishes(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    let (tx, rx) = mpsc::channel();
    view.update(cx, |s, cx| {
        s.job = Some(rx);
        cx.notify();
    });
    click(cx, "tab-Storage");
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let loading = cx.debug_bounds("storage-loading").unwrap();
        let content = cx.debug_bounds("settings-content").unwrap();
        assert!(loading.left() >= content.left() && loading.right() <= content.right());
        assert!(loading.top() >= content.top() && loading.bottom() <= content.bottom());
        assert!(cx.debug_bounds("clear-stems").is_none());
        assert!(cx.debug_bounds("storage-scan").is_none());
        view.read_with(cx, |s, _| {
            assert!(s.storage.is_none() && s.storage_scan == storage_page::ScanState::Queued);
        });
    }
    click(cx, "tab-General");
    click(cx, "tab-Storage");
    tx.send(JobResult::Devices(Ok(vec![]), Ok(vec![]))).unwrap();
    view.update(cx, |s, _| s.poll());
    view.read_with(cx, |s, _| {
        assert!(s.storage_scan == storage_page::ScanState::Running && s.job.is_some());
    });
    finish_storage_scan(&view, cx);
    assert!(cx.debug_bounds("clear-stems").is_some());
    assert!(cx.debug_bounds("storage-refresh").is_some());
}

fn finish_storage_scan(view: &gpui::Entity<Preferences>, cx: &mut VisualTestContext) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while view.read_with(cx, |s, _| s.storage_scan != storage_page::ScanState::Idle)
        && std::time::Instant::now() < deadline
    {
        view.update(cx, |s, cx| {
            s.poll();
            cx.notify();
        });
        std::thread::yield_now();
    }
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert!(s.storage.is_some() && s.storage_scan == storage_page::ScanState::Idle);
        assert!(!s.error);
    });
}

#[gpui::test]
fn failed_storage_scan_leaves_loading_and_can_retry(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    let (tx, rx) = mpsc::channel();
    view.update(cx, |s, cx| {
        s.tab = Tab::Storage;
        s.job = Some(rx);
        s.storage_scan = storage_page::ScanState::Running;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("storage-loading").is_some());
    tx.send(JobResult::Storage(Err(
        "Unable to read storage usage".into()
    )))
    .unwrap();
    view.update(cx, |s, cx| {
        s.poll();
        cx.notify();
    });
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert!(s.storage_scan == storage_page::ScanState::Idle && s.job.is_none());
        assert!(s.storage.is_none() && s.error && s.status == "Unable to read storage usage");
    });
    click(cx, "storage-scan");
    finish_storage_scan(&view, cx);
    // A stopped worker also exits loading instead of leaving a permanent spinner.
    let (tx, rx) = mpsc::channel();
    drop(tx);
    view.update(cx, |s, _| {
        s.storage = None;
        s.storage_scan = storage_page::ScanState::Running;
        s.job = Some(rx);
        s.poll();
        assert!(s.storage_scan == storage_page::ScanState::Idle && s.error);
        assert!(s.job.is_none());
    });
}

#[gpui::test]
fn every_settings_page_keeps_footer_and_controls_inside_small_and_large_windows(
    cx: &mut TestAppContext,
) {
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        s.storage = Some(StorageReport { stems: 4096, ..Default::default() });
        s.error = true;
        s.status = "A long device error should remain inside the status area without pushing Done off the window. ".repeat(8);
        cx.notify();
    });
    for (width, height) in [(720., 540.), (940., 820.), (1200., 900.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        for tab in [
            "tab-General",
            "tab-Audio I/O",
            "tab-Controllers",
            "tab-Storage",
        ] {
            click(cx, tab);
            let done = cx.debug_bounds("close-preferences").unwrap();
            assert!(
                done.left() > px(500.) && done.right() <= px(width) && done.bottom() <= px(height),
                "{tab}: {done:?}"
            );
            let content = cx.debug_bounds("settings-content").unwrap();
            assert!(content.top() >= px(98.) && content.bottom() <= done.top());
            let last_control = match tab {
                "tab-General" => "audio-folder-change",
                "tab-Audio I/O" => "audio-details",
                "tab-Controllers" => "mapping-list",
                _ => "storage-refresh",
            };
            scroll(
                cx,
                gpui::point(content.right() - px(3.), content.center().y),
                -5000.,
            );
            let last = cx.debug_bounds(last_control).unwrap();
            assert!(
                last.bottom() <= content.bottom() && last.right() <= content.right(),
                "{tab} bottom content: {last:?} in {content:?}"
            );
        }
    }
    let content = cx.debug_bounds("settings-content").unwrap();
    scroll(
        cx,
        gpui::point(content.right() - px(3.), content.center().y),
        5000.,
    );
    click(cx, "clear-stems");
    view.read_with(cx, |s, _| {
        assert!(s.confirm_clear == Some(ClearKind::Stems) && s.job.is_none())
    });
    cx.simulate_keystrokes("escape");
    view.read_with(cx, |s, _| assert!(s.confirm_clear.is_none()));
}

#[gpui::test]
fn mapping_workspace_and_options_keep_detail_out_of_the_primary_flow(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    // The layout stays above the mapping list; protocol notes and input
    // configuration remain optional.
    let diagram = cx.debug_bounds("controller-diagram").unwrap();
    assert!(cx.debug_bounds("midi-layout").is_none());
    assert!(cx.debug_bounds("controller-details").is_none());
    assert!(
        cx.debug_bounds("select-9").is_none(),
        "offline preview must not show an editable dummy MIDI address"
    );
    let list = cx.debug_bounds("mapping-list").unwrap();
    assert!(list.top() >= diagram.bottom(), "{list:?} {diagram:?}");
    click(cx, "select-11");
    let trigger = cx.debug_bounds("select-11").unwrap();
    let menu = cx.debug_bounds("preference-options").unwrap();
    assert_eq!(menu.size.width, trigger.size.width);
    assert_eq!(cx.debug_bounds("option-0").unwrap().size.height, px(30.));
    view.read_with(cx, |s, _| {
        let options = s.selector_options(Selector::MidiPreset);
        assert!(
            options
                .iter()
                .all(|(_, label)| !label.contains("Simulation")
                    && !label.contains("Protocol")
                    && !label.contains("Built-in"))
        );
    });
    click(cx, "option-1");
    view.read_with(cx, |s, _| {
        assert!(
            !s.preset_actions()
                .iter()
                .any(|(v, _)| v == "delete" || v == "rename")
        );
        assert!(s.hub().unwrap().assignments().is_empty());
    });
    let details_button = cx.debug_bounds("controller-more").unwrap();
    let manage_button = cx.debug_bounds("select-13").unwrap();
    assert_eq!(details_button.left(), manage_button.left());
    assert_eq!(details_button.right(), manage_button.right());
    assert_eq!(details_button.size.height, px(widgets::CONTROL_HEIGHT));
    click(cx, "controller-more");
    for (width, height) in [
        (720., 540.),
        (847., 700.),
        (848., 700.),
        (860., 700.),
        (940., 820.),
        (1200., 900.),
    ] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let choices = cx.debug_bounds("controller-choices").unwrap();
        let details = cx.debug_bounds("controller-details").unwrap();
        let panel = cx.debug_bounds("controller-panel").unwrap();
        if width >= 848. {
            assert!(details.left() >= choices.right() + px(24.));
            assert_eq!(details.top(), choices.top());
        } else {
            assert!(details.top() >= choices.bottom());
            assert_eq!(details.left(), choices.left());
        }
        assert!(details.right() <= panel.right(), "{details:?} in {panel:?}");
        let manage = cx.debug_bounds("select-13").unwrap();
        let more = cx.debug_bounds("controller-more").unwrap();
        assert_eq!(more.left(), manage.left());
        assert_eq!(more.right(), manage.right());
        let preset = cx.debug_bounds("select-11").unwrap();
        assert!(
            preset.size.width >= px(180.),
            "{width}: {preset:?}, {choices:?}, {details:?}, {panel:?}"
        );
    }
    cx.simulate_resize(size(px(940.), px(820.)));
    cx.run_until_parked();
    assert_eq!(cx.debug_bounds("controller-more").unwrap(), details_button);
    click(cx, "controller-more");
    let inputs_button = cx.debug_bounds("midi-inputs").unwrap();
    assert_eq!(inputs_button.size.height, details_button.size.height);
    click(cx, "midi-inputs");
    view.read_with(cx, |s, _| assert!(s.midi_inputs_open));
    assert_eq!(
        cx.debug_bounds("midi-inputs").unwrap().size,
        inputs_button.size
    );
    assert!(cx.debug_bounds("controller-diagram").unwrap().top() > diagram.top());
    click(cx, "midi-inputs");
    view.read_with(cx, |s, _| assert!(!s.midi_inputs_open));
    assert!(cx.debug_bounds("controller-diagram").is_some());
    click(cx, "diagram-Play { deck: A }");
    view.read_with(cx, |s, _| {
        assert_eq!(s.binding.target, MidiTarget::Play { deck: DeckId::A });
        assert!(
            !s.learning && s.midi_feedback.is_none(),
            "offline diagram browsing must not arm learning or produce an error"
        );
    });
    // Actions use the same anchored keyboard-accessible menu as selects.
    click(cx, "select-13");
    cx.simulate_keystrokes("home enter");
    view.read_with(cx, |s, _| {
        assert!(s.selector.is_none() && s.preset_dialog.is_some())
    });
    cx.simulate_keystrokes("escape");
    click(cx, "tab-Audio I/O");
    click(cx, "audio-details");
    view.read_with(cx, |s, _| assert!(s.audio_details_open));
    click(cx, "audio-details");
    view.read_with(cx, |s, _| assert!(!s.audio_details_open));
}

#[gpui::test]
fn device_selection_keeps_notes_and_packet_monitor_opt_in(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        s.controller_devices = (0..2)
            .map(|n| mixless_controllers::ControllerDevice {
                id: format!("ui-fixture-{n}"),
                name: format!("Test controller {}", n + 1),
                hid: false,
                state: mixless_controllers::ConnectionState::Connected,
                vendor_id: None,
                product_id: None,
                interface: None,
                usage_page: None,
                usage: None,
            })
            .collect();
        let h = s.hub().unwrap();
        let preset = h.presets().remove(0);
        h.assign(mixless_controllers::DeviceAssignment {
            device_id: "ui-fixture-0".into(),
            preset_id: preset.id,
            ports: [("main".into(), "ui-fixture-0".into())].into(),
            outputs: Default::default(),
        })
        .unwrap();
        cx.notify();
    });
    cx.run_until_parked();
    click(cx, "select-12");
    click(cx, "option-1");
    view.read_with(cx, |s, _| {
        assert_eq!(s.controller_device.as_deref(), Some("ui-fixture-0"));
        assert!(s.selected_profile().is_some());
        assert!(!s.controller_advanced && !s.controller_notes && !s.controller_monitor);
    });
    assert!(cx.debug_bounds("controller-notes-text").is_none());
    assert!(cx.debug_bounds("controller-monitor-content").is_none());
    click(cx, "controller-more");
    assert!(cx.debug_bounds("controller-notes-text").is_none());
    assert!(cx.debug_bounds("controller-monitor-content").is_none());
    click(cx, "controller-notes");
    assert!(
        cx.debug_bounds("controller-notes-text")
            .unwrap()
            .size
            .height
            <= px(120.)
    );
    click(cx, "controller-monitor");
    assert!(cx.debug_bounds("controller-monitor-content").is_some());
    click(cx, "select-11");
    click(cx, "option-2");
    view.read_with(cx, |s, _| {
        assert!(!s.controller_advanced && !s.controller_notes && !s.controller_monitor)
    });
    click(cx, "controller-more");
    click(cx, "controller-notes");
    click(cx, "select-12");
    click(cx, "option-2");
    view.read_with(cx, |s, _| {
        assert_eq!(s.controller_device.as_deref(), Some("ui-fixture-1"));
        assert!(!s.controller_advanced && !s.controller_notes && !s.controller_monitor);
        assert!(s.selected_preset.is_none());
        assert_eq!(
            s.hub().unwrap().assignments().len(),
            1,
            "browsing must not change assignments"
        );
    });
    // The editor's source selector follows the same device context rules.
    view.update(cx, |s, cx| {
        s.controller_advanced = true;
        s.controller_notes = true;
        s.controller_monitor = true;
        s.choose(Selector::Source, "ui-fixture-0", cx);
        assert!(!s.controller_advanced && !s.controller_notes && !s.controller_monitor);
        assert!(s.selected_profile().is_some());
    });
}

#[gpui::test]
fn mapping_switcher_keeps_the_inspector_in_its_selected_group(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    for (width, height) in [(720., 540.), (940., 820.), (1200., 900.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let content = cx.debug_bounds("settings-content").unwrap();
        let page_edge = gpui::point(content.right() - px(3.), content.center().y);
        let switcher = cx.debug_bounds("mapping-group-switcher").unwrap();
        scroll(
            cx,
            page_edge,
            f32::from(content.top() + px(12.) - switcher.top()),
        );
        let switcher = cx.debug_bounds("mapping-group-switcher").unwrap();
        assert!(switcher.left() >= content.left() && switcher.right() <= content.right());
        assert!(
            switcher.top() >= content.top() && switcher.bottom() <= content.bottom(),
            "{switcher:?} in {content:?}"
        );
        for (id, group) in [
            ("mapping-group-2", MidiGroup::DeckA),
            ("mapping-group-3", MidiGroup::DeckB),
            ("mapping-group-4", MidiGroup::Library),
            ("mapping-group-1", MidiGroup::Mixer),
        ] {
            click(cx, id);
            view.read_with(cx, |s, _| {
                assert_eq!(s.midi_group, Some(group));
                assert_eq!(s.binding.target.group(), group);
                assert!(!s.learning);
            });
        }
        click(cx, "mapping-group-0");
        view.read_with(cx, |s, _| assert!(s.midi_group.is_none()));
        scroll(cx, page_edge, 5000.);
        for id in ["select-11", "select-12", "select-13", "controller-apply"] {
            let control = cx.debug_bounds(id).unwrap();
            assert_eq!(control.size.height, px(32.));
            assert!(control.right() <= content.right());
        }
    }
}

#[gpui::test]
fn complex_device_mapping_shows_its_real_input_and_edits_the_assigned_preset(
    cx: &mut TestAppContext,
) {
    let (_temp, view, cx) = fixture(cx);
    let assigned = view.update(cx, |s, cx| {
        let h = s.hub().unwrap();
        let preset = h.presets().remove(0);
        h.assign(mixless_controllers::DeviceAssignment {
            device_id: "ui-preview-input".into(),
            preset_id: preset.id.clone(),
            ports: [("main".into(), "ui-preview-input".into())].into(),
            outputs: Default::default(),
        })
        .unwrap();
        s.bindings = h.bindings();
        s.controller_device = Some("ui-preview-input".into());
        // A different preset may be selected without applying it.
        s.selected_preset = Some(h.presets()[1].id.clone());
        s.select_midi_target(MidiTarget::Play { deck: DeckId::A });
        assert!(
            s.editing.is_none(),
            "fixture must exercise a layered preset input"
        );
        cx.notify();
        preset.id
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("mapping-preview").is_some());
    assert!(
        cx.debug_bounds("select-9").is_none(),
        "must not show a dummy Note 0"
    );
    let content = cx.debug_bounds("settings-content").unwrap();
    let page_edge = gpui::point(content.right() - px(3.), content.center().y);
    scroll(cx, page_edge, -5000.);
    click(cx, "midi-preview-learn");
    // This render fixture is not a connected transport: learning must fail
    // safely and leave the assigned mapping intact.
    view.read_with(cx, |s, _| {
        assert!(!s.learning);
        assert!(s.midi_feedback.as_ref().is_some_and(|(_, error)| *error));
        assert_eq!(s.hub().unwrap().assignments()[0].preset_id, assigned);
    });
    scroll(cx, page_edge, -5000.);
    click(cx, "edit-preset-mapping");
    view.read_with(cx, |s, _| {
        assert_eq!(s.selected_preset.as_ref(), Some(&assigned));
        assert!(s.preset_dialog.is_some());
    });
}
