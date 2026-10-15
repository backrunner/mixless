use super::*;

fn previous_audio() -> AudioConfig {
    AudioConfig {
        master_device: Some("Studio output".into()),
        master_uid: Some("studio-uid".into()),
        cue_device: Some("Headphone output".into()),
        cue_uid: Some("headphone-uid".into()),
        master_channels: [4, 5],
        cue_channels: [6, 7],
        sample_rate: Some(48_000),
        bit_depth: Some(24),
        buffer_frames: Some(512),
    }
}

fn routing_preset(view: &gpui::Entity<Preferences>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |s, _| {
        s.hub()
            .unwrap()
            .presets()
            .into_iter()
            .find(|p| p.name == "DDJ-FLX4")
            .filter(|p| p.audio.is_some())
            .expect("FLX4 audio recommendation")
            .id
    })
}

#[gpui::test]
fn none_preset_restores_the_audio_routing_draft(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    let preset = routing_preset(&view, cx);
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        view.update(cx, |s, cx| {
            s.audio = previous_audio();
            s.choose(Selector::MidiPreset, &preset, cx);
            s.tab = Tab::Midi;
            s.controller_advanced = true;
            cx.notify();
        });
        cx.run_until_parked();
        let button = cx.debug_bounds("controller-audio").unwrap();
        if button.center().y > px(height - 100.) {
            let content = cx.debug_bounds("settings-content").unwrap();
            scroll(
                cx,
                content.center(),
                height - 140. - f32::from(button.center().y),
            );
        }
        click(cx, "controller-audio");
        view.read_with(cx, |s, _| {
            assert!(s.tab == Tab::Audio);
            assert_eq!(s.audio.master_device.as_deref(), Some("DDJ-FLX4"));
            assert_eq!(s.audio.cue_device.as_deref(), Some("DDJ-FLX4"));
            assert_eq!(s.core.settings.get().audio, AudioConfig::default());
            assert_eq!(s.core.engine.audio_config(), AudioConfig::default());
        });
        click(cx, "tab-Controllers");
        click(cx, "select-11");
        cx.simulate_keystrokes("home enter");
        cx.run_until_parked();
        view.read_with(cx, |s, _| {
            assert!(s.selected_preset.is_none());
            assert_eq!(
                s.audio,
                previous_audio(),
                "None retained the controller route"
            );
            assert_eq!(s.core.settings.get().audio, AudioConfig::default());
            assert_eq!(s.core.engine.audio_config(), AudioConfig::default());
        });
    }
}

#[gpui::test]
fn repeated_routing_and_preset_or_device_changes_restore_the_original_route(
    cx: &mut TestAppContext,
) {
    let (_temp, view, cx) = fixture(cx);
    let preset = routing_preset(&view, cx);
    view.update(cx, |s, cx| {
        s.audio = previous_audio();
        s.choose(Selector::MidiPreset, &preset, cx);
        s.apply_recommended_audio();
        s.apply_recommended_audio();
        s.choose(Selector::MidiPreset, "builtin-ddj-400-v1", cx);
        assert_eq!(s.audio, previous_audio());
        s.apply_recommended_audio();
        assert_eq!(s.audio.master_device.as_deref(), Some("DDJ-400"));
        s.choose(Selector::ControllerDevice, "another-device", cx);
        assert_eq!(s.audio, previous_audio());
        assert!(s.controller_audio_draft.is_none());

        // NI can recommend audio without a MIDI preset. None must still undo it.
        s.controller_devices = vec![mixless_controllers::ControllerDevice {
            id: "ni-audio-device".into(),
            name: "Traktor Kontrol S4 MK3".into(),
            hid: false,
            state: mixless_controllers::ConnectionState::Offline,
            vendor_id: None,
            product_id: None,
            interface: None,
            usage_page: None,
            usage: None,
        }];
        s.controller_device = Some("ni-audio-device".into());
        s.selected_preset = None;
        s.apply_recommended_audio();
        assert_eq!(
            s.audio.master_device.as_deref(),
            Some("Traktor Kontrol S4 MK3")
        );
        s.choose(Selector::MidiPreset, "", cx);
        assert_eq!(s.audio, previous_audio());
    });
}

#[gpui::test]
fn clearing_a_routing_draft_preserves_manual_output_and_format_edits(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    let preset = routing_preset(&view, cx);
    view.update(cx, |s, cx| {
        for edited_output in [Selector::Master, Selector::Cue] {
            s.audio = previous_audio();
            s.choose(Selector::MidiPreset, &preset, cx);
            s.apply_recommended_audio();
            s.choose(edited_output, "Manual output", cx);
            s.choose(Selector::Rate, "96000", cx);
            s.choose(Selector::Depth, "16", cx);
            s.choose(Selector::Buffer, "128", cx);
            let mut expected = s.audio.clone();
            let previous = previous_audio();
            if edited_output == Selector::Master {
                expected.cue_device = previous.cue_device;
                expected.cue_uid = previous.cue_uid;
                expected.cue_channels = previous.cue_channels;
            } else {
                expected.master_device = previous.master_device;
                expected.master_uid = previous.master_uid;
                expected.master_channels = previous.master_channels;
            }
            s.choose(Selector::MidiPreset, "", cx);
            assert_eq!(s.audio, expected);
        }
    });
}

#[gpui::test]
fn failed_audio_apply_leaves_an_unsaved_recommendation_reversible(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    let preset = routing_preset(&view, cx);
    view.update(cx, |s, cx| {
        s.audio = previous_audio();
        s.choose(Selector::MidiPreset, &preset, cx);
        s.apply_recommended_audio();
        // Deliver worker completion messages without opening hardware in this test.
        let (tx, rx) = mpsc::channel();
        s.job = Some(rx);
        tx.send(JobResult::Audio(Err("Output unavailable".into())))
            .unwrap();
        s.poll();
        assert!(s.controller_audio_draft.is_some());
        assert!(s.error);
        s.choose(Selector::MidiPreset, "", cx);
        assert_eq!(s.audio, previous_audio());
    });
}

fn deliver_audio_result(s: &mut Preferences, result: JobResult) {
    let (tx, rx) = mpsc::channel();
    s.job = Some(rx);
    tx.send(result).unwrap();
    s.poll();
}

#[gpui::test]
fn saved_recommendation_restores_outputs_after_none_and_reopening_preferences(
    cx: &mut TestAppContext,
) {
    let (temp, view, cx) = fixture(cx);
    let preset = routing_preset(&view, cx);
    view.update(cx, |s, cx| {
        s.audio = AudioConfig::default();
        s.choose(Selector::MidiPreset, &preset, cx);
        s.apply_recommended_audio();
        let suggested = s.audio.clone();
        let backup = s.controller_audio_backup(&suggested).unwrap();
        assert_eq!(backup.previous, AudioConfig::default());
        let mut simulated_device = AudioConfig::default();
        // Exercise the production save transaction with a simulated device.
        audio::save_audio_settings_with(
            &s.core.settings,
            simulated_device.clone(),
            suggested.clone(),
            Some(backup),
            |config| {
                simulated_device = config;
                Ok(())
            },
        )
        .unwrap();
        deliver_audio_result(s, JobResult::Audio(Ok(())));
        assert!(s.controller_audio_draft.is_some());
        assert_eq!(simulated_device, suggested);
        let disk = crate::settings::SettingsStore::load(temp.path().join("preferences.json"));
        assert_eq!(disk.get(), s.core.settings.get());
        let saved = disk.get().controller_audio_restore.unwrap();
        // A fresh Preferences view reconstructs the disk-backed route history.
        let reopened = Preferences::new(s.core.clone(), cx);
        assert!(reopened.controller_audio_draft.is_some());
        s.controller_audio_draft = reopened.controller_audio_draft;
        // Unsaved format changes must not be committed by the restore.
        s.choose(Selector::Buffer, "128", cx);
        s.choose(Selector::MidiPreset, "", cx);
        assert!(s.job.is_some() && s.controller_audio_restore.is_some());
        assert!(s.selected_preset.is_none());
        assert_eq!(s.audio.master_device, None);
        assert_eq!(s.audio.cue_device, None);
        assert_eq!(s.audio.buffer_frames, Some(128));
        assert_eq!(s.core.settings.get().audio, suggested);
        // The real offline-engine job cannot open devices. Substitute its
        // completion with the same transaction against our simulated device.
        audio::save_audio_settings_with(
            &s.core.settings,
            simulated_device.clone(),
            saved.previous.clone(),
            None,
            |config| {
                simulated_device = config;
                Ok(())
            },
        )
        .unwrap();
        deliver_audio_result(s, JobResult::ControllerAudioRestored(Ok(())));
        assert!(!s.error && s.job.is_none());
        assert!(s.controller_audio_draft.is_none() && s.controller_audio_restore.is_none());
        assert_eq!(simulated_device, AudioConfig::default());
        assert_eq!(s.core.settings.get().audio, AudioConfig::default());
        assert_eq!(s.audio.buffer_frames, Some(128));
        let disk = crate::settings::SettingsStore::load(temp.path().join("preferences.json"));
        assert_eq!(disk.get().audio, AudioConfig::default());
        assert!(disk.get().controller_audio_restore.is_none());
    });
}

#[gpui::test]
fn restoration_failure_recovers_selection_and_keeps_the_backup_for_retry(cx: &mut TestAppContext) {
    let (temp, view, cx) = fixture(cx);
    let preset = routing_preset(&view, cx);
    view.update(cx, |s, cx| {
        let suggested = AudioConfig {
            master_device: Some("DDJ-FLX4".into()),
            cue_device: Some("DDJ-FLX4".into()),
            master_channels: [0, 1],
            cue_channels: [2, 3],
            ..Default::default()
        };
        let restored = AudioConfig {
            sample_rate: None,
            bit_depth: None,
            buffer_frames: None,
            ..previous_audio()
        };
        let backup = crate::settings::ControllerAudioRestore {
            previous: previous_audio(),
            suggested: suggested.clone(),
        };
        s.core
            .settings
            .update(|settings| {
                settings.audio = suggested.clone();
                settings.controller_audio_restore = Some(backup.clone());
            })
            .unwrap();
        s.audio = suggested.clone();
        s.controller_audio_draft = Some(controllers::ControllerAudioDraft::from_saved(
            backup.clone(),
        ));
        s.selected_preset = Some(preset.clone());
        s.choose(Selector::MidiPreset, "", cx);
        let error = audio::save_audio_settings_with(
            &s.core.settings,
            suggested.clone(),
            restored.clone(),
            None,
            |_| Err("Original output disconnected".into()),
        );
        deliver_audio_result(s, JobResult::ControllerAudioRestored(error));
        assert!(s.error);
        assert_eq!(s.selected_preset.as_deref(), Some(preset.as_str()));
        assert_eq!(s.audio, suggested);
        assert!(s.controller_audio_draft.is_some());
        assert_eq!(
            s.core.settings.get().controller_audio_restore,
            Some(backup.clone())
        );

        s.choose(Selector::MidiPreset, "", cx);
        let path = s.core.settings.path.clone();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let mut device = suggested.clone();
        let mut calls = Vec::new();
        let result = audio::save_audio_settings_with(
            &s.core.settings,
            suggested.clone(),
            restored.clone(),
            None,
            |config| {
                calls.push(config.clone());
                device = config;
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, [restored.clone(), suggested.clone()]);
        assert_eq!(
            device, suggested,
            "Persist failure must roll the device back"
        );
        deliver_audio_result(s, JobResult::ControllerAudioRestored(result));
        assert!(s.error && s.selected_preset.is_some());
        assert_eq!(s.core.settings.get().audio, suggested);
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, bytes).unwrap();

        s.choose(Selector::MidiPreset, "", cx);
        let result = audio::save_audio_settings_with(
            &s.core.settings,
            suggested.clone(),
            restored.clone(),
            None,
            |config| {
                device = config;
                Ok(())
            },
        );
        deliver_audio_result(s, JobResult::ControllerAudioRestored(result));
        assert!(!s.error && s.selected_preset.is_none());
        assert_eq!(device, restored.clone());
        assert_eq!(s.audio, restored.clone());
        assert!(s.core.settings.get().controller_audio_restore.is_none());
        let disk = crate::settings::SettingsStore::load(temp.path().join("preferences.json"));
        assert_eq!(disk.get().audio, restored.clone());
        assert!(disk.get().controller_audio_restore.is_none());
    });
}
