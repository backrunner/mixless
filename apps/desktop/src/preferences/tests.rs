//! Interaction/layout regressions using the actual Preferences render tree.
use super::*;
use gpui::{TestAppContext, VisualTestContext};

fn fixture(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    gpui::Entity<Preferences>,
    &mut VisualTestContext,
) {
    let (temp, core, _) = crate::automix::tests::fixture();
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
    let (_temp, _view, cx) = fixture(cx);
    click(cx, "midi-layout");
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
    }
}

#[gpui::test]
fn preset_selection_is_offline_and_modal_blocks_background_clicks(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    // Export is unavailable until a preset is chosen.
    view.read_with(cx, |s, _| {
        assert!(!s.preset_actions().iter().any(|(v, _)| v == "export"))
    });
    view.read_with(cx, |s, _| assert!(s.job.is_none() && !s.error));
    click(cx, "select-11");
    view.read_with(cx, |s, _| {
        assert_eq!(s.selector_options(Selector::MidiPreset).len(), 12)
    });
    click(cx, "option-0");
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
fn import_retry_clears_previous_error_and_cancel_keeps_library(cx: &mut TestAppContext) {
    let (temp, view, cx) = fixture(cx);
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
        assert_eq!(s.hub().unwrap().presets().len(), 12);
    });
    deliver(&view, cx, Ok(report));
    click(cx, "accept-preset-import");
    view.read_with(cx, |s, _| {
        assert!(!s.selected_profile().unwrap().is_builtin());
        assert!(s.hub().unwrap().assignments().is_empty());
        assert_eq!(s.hub().unwrap().presets().len(), 13);
    });
    preset_action(&view, cx, "edit");
    cx.simulate_keystrokes("cmd-a");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("{bad json".into()));
    cx.simulate_keystrokes("cmd-v cmd-enter");
    view.read_with(cx, |s, _| {
        assert!(s.error && s.preset_dialog.is_some());
        assert_eq!(s.selected_profile().unwrap().name, "DDJ-400");
        assert_eq!(s.hub().unwrap().presets().len(), 13);
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
    // The default page puts the mapping workspace above the fold, with optional
    // diagram, protocol notes and input configuration closed.
    assert!(cx.debug_bounds("controller-diagram").is_none());
    assert!(cx.debug_bounds("controller-details").is_none());
    assert!(
        cx.debug_bounds("select-9").is_none(),
        "offline preview must not show an editable dummy MIDI address"
    );
    let list = cx.debug_bounds("mapping-list").unwrap();
    assert!(
        list.bottom() < cx.debug_bounds("close-preferences").unwrap().top(),
        "{list:?}"
    );
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
    click(cx, "option-0");
    view.read_with(cx, |s, _| {
        assert!(
            !s.preset_actions()
                .iter()
                .any(|(v, _)| v == "delete" || v == "rename")
        );
        assert!(s.hub().unwrap().assignments().is_empty());
    });
    click(cx, "controller-more");
    assert!(cx.debug_bounds("controller-details").is_some());
    click(cx, "controller-more");
    click(cx, "midi-layout");
    assert!(cx.debug_bounds("controller-diagram").is_some());
    click(cx, "diagram-Play { deck: A }");
    view.read_with(cx, |s, _| {
        assert_eq!(s.binding.target, MidiTarget::Play { deck: DeckId::A });
        assert!(
            !s.learning && s.midi_feedback.is_none(),
            "offline diagram browsing must not arm learning or produce an error"
        );
    });
    let expanded_list = cx.debug_bounds("mapping-list").unwrap();
    click(cx, "midi-layout");
    view.read_with(cx, |s, _| assert!(!s.midi_layout_open));
    assert!(cx.debug_bounds("mapping-list").unwrap().top() < expanded_list.top());
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
    click(cx, "option-1");
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
        let switcher = cx.debug_bounds("mapping-group-switcher").unwrap();
        let content = cx.debug_bounds("settings-content").unwrap();
        assert!(switcher.left() >= content.left() && switcher.right() <= content.right());
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
    click(cx, "midi-preview-learn");
    // This render fixture is not a connected transport: learning must fail
    // safely and leave the assigned mapping intact.
    view.read_with(cx, |s, _| {
        assert!(!s.learning);
        assert!(s.midi_feedback.as_ref().is_some_and(|(_, error)| *error));
        assert_eq!(s.hub().unwrap().assignments()[0].preset_id, assigned);
    });
    click(cx, "edit-preset-mapping");
    view.read_with(cx, |s, _| {
        assert_eq!(s.selected_preset.as_ref(), Some(&assigned));
        assert!(s.preset_dialog.is_some());
    });
}
