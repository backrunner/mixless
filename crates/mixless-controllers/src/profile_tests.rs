use crate::profile::PresetStore;
use crate::router::Router;
use crate::*;
use mixless_protocol::{DeckId, EngineSnapshot};
use std::{collections::BTreeMap, fs, path::PathBuf};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(new_id());
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn manager(t: &Temp) -> ControllerManager {
    ControllerManager::start_with_config(
        t.0.join("midi.json"),
        MidiConfig {
            enabled: false,
            input_ids: None,
        },
    )
    .unwrap()
}
fn binding(target: MidiTarget) -> Binding {
    Binding::from_legacy(&MidiBinding {
        device_id: None,
        kind: MidiSourceKind::Cc,
        channel: 2,
        number: 17,
        target,
        mode: MidiControlMode::Auto,
    })
}
fn assign(h: &ControllerManager, preset: &str, device: &str) {
    h.assign(DeviceAssignment {
        device_id: device.into(),
        preset_id: preset.into(),
        ports: BTreeMap::from([("main".into(), device.into())]),
        outputs: Default::default(),
    })
    .unwrap();
}
#[test]
fn catalogue_has_independent_sources_and_valid_complete_packets() {
    let p = builtin_presets();
    assert_eq!(p.len(), 12);
    let mut ids = std::collections::BTreeSet::new();
    for p in p {
        p.validate().unwrap_or_else(|e| panic!("{}: {e}", p.name));
        assert!(ids.insert(p.id.clone()));
        assert_eq!(p.verification, Verification::Protocol);
        let domain = match p.manufacturer.as_str() {
            "Pioneer DJ / AlphaTheta" => "https://downloads.support.alphatheta.com/",
            "Hercules" => "https://ts.hercules.com/",
            "Numark" => "https://www.numark.com/",
            other => panic!("Unreviewed manufacturer source: {other}"),
        };
        assert!(!p.provenance.is_empty());
        assert!(p
            .provenance
            .iter()
            .all(|s| s.url.starts_with(domain) && !s.revision.is_empty() && !s.section.is_empty()));
        assert!(!p.notes.is_empty());
        assert!(p.bindings.len() > 20, "{}", p.name);
        assert!(p.feedback.len() >= 8, "{}", p.name);
        assert!(p.matches_midi(&p.name));
        assert!(!p.matches_midi("Unrelated keyboard"));
        assert!(!p.matches_midi(&format!("{} MK2", p.name)));
        // Sharing preserves coverage notes but never introduces local device IDs.
        let bytes = serde_json::to_vec(&p).unwrap();
        assert_eq!(
            serde_json::from_slice::<ControllerPreset>(&bytes).unwrap(),
            p
        );
    }
}

fn builtin(name: &str) -> ControllerPreset {
    builtin_presets()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
}

#[test]
fn catalogue_routes_both_decks_and_temporary_cue_edges() {
    for p in builtin_presets() {
        for deck in [DeckId::A, DeckId::B] {
            for (target, down, up) in [
                (MidiTarget::Play { deck }, MidiValue::Press, None),
                (
                    MidiTarget::TemporaryCue { deck },
                    MidiValue::Hold,
                    Some(MidiValue::Release),
                ),
            ] {
                let b = p.bindings.iter().find(|b| b.target == target).unwrap();
                let InputSource::Midi {
                    address: MidiAddress::Note { channel, number },
                } = b.source
                else {
                    panic!("{}", p.name)
                };
                let mut r = Router::default();
                let snapshot = EngineSnapshot::default();
                let actions = r.route(
                    "unit",
                    "main",
                    &p,
                    false,
                    &[0x90 | channel, number, 127],
                    &snapshot,
                );
                assert_eq!(actions.len(), 1, "{} {target:?}", p.name);
                assert_eq!(actions[0].target, target);
                assert_eq!(actions[0].value, down);
                let actions = r.route(
                    "unit",
                    "main",
                    &p,
                    false,
                    &[0x80 | channel, number, 64],
                    &snapshot,
                );
                assert_eq!(
                    actions.first().map(|a| a.value),
                    up,
                    "{} {target:?}",
                    p.name
                );
            }
        }
    }
}

#[test]
fn catalogue_keeps_brand_and_generation_specific_messages() {
    let s = EngineSnapshot::default();
    let mut r = Router::default();
    // SB3's right pads are on channel 9; DDJ-400's right pads are on channel 10.
    let sb3 = builtin("DDJ-SB3");
    let ddj400 = builtin("DDJ-400");
    assert_eq!(
        r.route("sb3", "main", &sb3, false, &[0x98, 0, 127], &s)[0].target,
        MidiTarget::Cue {
            deck: DeckId::B,
            index: 0
        }
    );
    assert!(r
        .route("400", "main", &ddj400, false, &[0x98, 0, 127], &s)
        .is_empty());
    assert_eq!(
        r.route("400", "main", &ddj400, false, &[0x99, 0, 127], &s)[0].target,
        MidiTarget::Cue {
            deck: DeckId::B,
            index: 0
        }
    );
    // A WeGO3 right fader belongs to the global channel, not the right deck channel.
    let wego = builtin("DDJ-WeGO3");
    r.route("wego", "main", &wego, false, &[0xb6, 0x15, 64], &s);
    let a = r.route("wego", "main", &wego, false, &[0xb6, 0x35, 1], &s);
    assert_eq!(a[0].target, MidiTarget::Fader { deck: DeckId::B });
    assert!(r
        .route("wego", "main", &wego, false, &[0xb1, 0x13, 64], &s)
        .is_empty());
    let mut s = s;
    s.decks[0].src_sample_rate = 48000;
    for (name, message) in [
        ("DJControl Inpulse 200", [0xb1, 9, 127]),
        ("DJControl Inpulse 300", [0xb1, 9, 127]),
        ("DJ2GO", [0xb0, 0x19, 127]),
    ] {
        let a = r.route(name, "main", &builtin(name), false, &message, &s);
        assert_eq!(a[0].target, MidiTarget::Jog { deck: DeckId::A });
        assert!(
            matches!(a[0].value, MidiValue::JogFrames(v) if v < 0.),
            "{name}"
        );
    }
    // No bank-wide wildcard, MK2 reuse or invented hardware master-volume messages.
    for name in ["DDJ-SB", "DDJ-SB2"] {
        let p = builtin(name);
        assert!(!p
            .bindings
            .iter()
            .any(|b| b.target == MidiTarget::Master && b.sets_layer.is_none()));
        assert_eq!(p.initialize[0].data, [0x9b, 9, 0]);
    }
    assert!(!builtin("DJ2GO").matches_midi("DJ2GO2 Touch"));
    assert!(builtin("DDJ-200").audio.is_none());
}

#[test]
fn shift_changes_release_holds_and_reset_button_edges_on_only_that_unit() {
    let p = builtin("DDJ-SB3");
    let s = EngineSnapshot::default();
    let mut r = Router::default();
    r.route("unit-a", "main", &p, false, &[0x90, 0x0b, 127], &s);
    r.route("unit-b", "main", &p, false, &[0x90, 0x0b, 127], &s);
    r.route("unit-a", "main", &p, false, &[0x90, 0x36, 127], &s);
    let releases = r.route("unit-a", "main", &p, false, &[0x90, 0x3f, 127], &s);
    assert!(releases
        .iter()
        .any(|a| a.target == MidiTarget::JogTouch { deck: DeckId::A }
            && a.value == MidiValue::Release));
    // Firmware sends release on a shifted address that has no normal Play binding.
    r.route("unit-a", "main", &p, false, &[0x90, 0x47, 0], &s);
    r.route("unit-a", "main", &p, false, &[0x90, 0x3f, 0], &s);
    assert_eq!(
        r.route("unit-a", "main", &p, false, &[0x90, 0x0b, 127], &s)
            .len(),
        1
    );
    assert!(r
        .route("unit-b", "main", &p, false, &[0x90, 0x0b, 127], &s)
        .is_empty());
    assert!(r.release_device("unit-a").is_empty());
}
#[test]
fn migration_keeps_original_backup_assignments_and_explicit_wildcard() {
    let t = Temp::new();
    let file = t.0.join("midi.json");
    let mut b = binding(MidiTarget::Master)
        .legacy(Some("old-port".into()))
        .unwrap();
    let mut wildcard = b.clone();
    wildcard.device_id = None;
    wildcard.number = 1;
    save_map(&file, &[b.clone(), wildcard.clone()]).unwrap();
    let bytes = fs::read(&file).unwrap();
    let store = PresetStore::open(&file).unwrap();
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert_eq!(
        fs::read(file.with_extension("pre-controllers.backup.json")).unwrap(),
        bytes
    );
    assert!(store.flattened().contains(&b));
    assert_eq!(store.state.wildcard, vec![wildcard]);
    assert_eq!(store.state.assignments.len(), 1);
    b.number = 7;
    save_map(&file, &[b]).unwrap();
    assert_eq!(PresetStore::open(&file).unwrap().state.assignments.len(), 1); // one-time migration
}
#[test]
fn portable_crud_rollback_restart_and_device_isolation() {
    let t = Temp::new();
    let h = manager(&t);
    let mut p = ControllerPreset::blank("Two units");
    p.bindings.push(binding(MidiTarget::Master));
    let id = h.save_preset(p).unwrap();
    assign(&h, &id, "a");
    assign(&h, &id, "b");
    let mut edit = h.preset(&id).unwrap();
    edit.bindings[0].target = MidiTarget::CueGain;
    let new = h.save_device_preset(edit, Some("a")).unwrap();
    assert_ne!(new, id);
    assign(&h, &new, "a");
    assert_eq!(
        h.assignments()
            .iter()
            .find(|a| a.device_id == "b")
            .unwrap()
            .preset_id,
        id
    );
    assert_eq!(
        h.preset(&id).unwrap().bindings[0].target,
        MidiTarget::Master
    );
    h.rename_preset(&new, "Custom name").unwrap();
    let copy = h.duplicate_preset(&new, "Portable").unwrap();
    let export = t.0.join("portable.json");
    h.export_preset(&copy, &export).unwrap();
    let json = fs::read_to_string(&export).unwrap();
    assert!(!json.contains("device_id"));
    let other = Temp::new();
    let h2 = manager(&other);
    let report = h2.preview_import(&export).unwrap();
    assert!(h2.assignments().is_empty());
    let imported = h2.accept_import(report).unwrap();
    assert_eq!(h2.preset(&imported).unwrap().name, "Portable");
    assert!(h2.assignments().is_empty());
    fs::write(&export, json.replace("\"version\": 1", "\"version\": 999")).unwrap();
    let before = h2.presets();
    assert!(h2.preview_import(&export).is_err());
    assert_eq!(h2.presets(), before);
    h.delete_preset(&copy).unwrap();
    assert!(h.delete_preset(&id).is_err());
    let state = fs::read(h.storage_path()).unwrap();
    fs::remove_file(h.storage_path()).unwrap();
    fs::create_dir(h.storage_path()).unwrap();
    assert!(h.rename_preset(&new, "Lost update").is_err());
    assert_eq!(h.preset(&new).unwrap().name, "Custom name");
    fs::remove_dir(h.storage_path()).unwrap();
    fs::write(h.storage_path(), state).unwrap();
    drop(h);
    let h = manager(&t);
    assert_eq!(h.preset(&new).unwrap().name, "Custom name");
    assert_eq!(h.assignments().len(), 2);
}
#[test]
fn invalid_reports_feedback_and_duplicate_inputs_rejected() {
    let mut p = ControllerPreset::blank("bad");
    let mut b = binding(MidiTarget::Master);
    b.source = InputSource::Hid {
        report_id: 0,
        bit_offset: 8,
        bits: 0,
        signed: false,
        little_endian: true,
    };
    p.bindings.push(b);
    assert!(p.validate().is_err());
    let mut p = builtin_presets().remove(0);
    p.feedback[0].bits = 8;
    assert!(p.validate().is_err());
    let mut p = ControllerPreset::blank("duplicates");
    p.bindings.push(binding(MidiTarget::Master));
    let mut b = p.bindings[0].clone();
    b.id = "another".into();
    p.bindings.push(b);
    assert!(p.validate().is_err());
}
#[test]
fn hid_reports_signed_counters_layers_pickup_and_disconnect() {
    let mut p = ControllerPreset::blank("HID fixture");
    p.endpoints[0].matcher = DeviceMatcher::Hid {
        vendor_id: 0x1209,
        product_id: 1,
        interface: Some(0),
        usage_page: Some(0xff00),
        usage: Some(1),
    };
    let mut b = binding(MidiTarget::Jog { deck: DeckId::A });
    b.source = InputSource::Hid {
        report_id: 3,
        bit_offset: 8,
        bits: 8,
        signed: false,
        little_endian: true,
    };
    b.mode = ValueMode::Counter;
    b.ticks_per_revolution = Some(256.);
    p.bindings.push(b);
    let mut r = Router::default();
    let mut snap = EngineSnapshot::default();
    snap.decks[0].src_sample_rate = 48000;
    assert!(r.route("a", "main", &p, true, &[3, 254], &snap).is_empty());
    let actions = r.route("a", "main", &p, true, &[3, 1], &snap);
    assert_eq!(
        actions[0].value,
        MidiValue::JogFrames(3. * 48000. * 1.8 / 256.)
    );
    assert!(r.route("b", "main", &p, true, &[3, 1], &snap).is_empty());
    assert!(r.route("a", "main", &p, true, &[4, 9], &snap).is_empty());
    let mut p = ControllerPreset::blank("Layers");
    let mut modifier = binding(MidiTarget::Master);
    modifier.sets_layer = Some("shift".into());
    modifier.id = "shift".into();
    let mut hold = binding(MidiTarget::JogTouch { deck: DeckId::A });
    hold.id = "touch".into();
    hold.source = InputSource::Midi {
        address: MidiAddress::Note {
            channel: 0,
            number: 5,
        },
    };
    hold.layer = Some(LayerCondition {
        name: "shift".into(),
        active: false,
    });
    p.bindings = vec![modifier, hold];
    assert_eq!(
        r.route("a", "main", &p, false, &[0x90, 5, 127], &snap)[0].value,
        MidiValue::Press
    );
    assert_eq!(
        r.route("a", "main", &p, false, &[0xb2, 17, 127], &snap)[0].value,
        MidiValue::Release
    );
    assert!(r.release_device("a").is_empty());
    // A modifier on one logical port controls a binding on another interface.
    r.route_for_controller(
        "controller",
        "buttons",
        "main",
        &p,
        false,
        &[0xb2, 17, 127],
        &snap,
    );
    assert!(r
        .route_for_controller(
            "controller",
            "jog",
            "main",
            &p,
            false,
            &[0x90, 5, 127],
            &snap
        )
        .is_empty());
    let mut p = ControllerPreset::blank("Pickup");
    let mut b = binding(MidiTarget::Master);
    b.pickup = true;
    p.bindings.push(b);
    snap.master = 0.5;
    assert!(r
        .route("a", "main", &p, false, &[0xb2, 17, 10], &snap)
        .is_empty());
    assert_eq!(
        r.route("a", "main", &p, false, &[0xb2, 17, 70], &snap)
            .len(),
        1
    );
    assert!(r
        .route("b", "main", &p, false, &[0xb2, 17, 10], &snap)
        .is_empty());
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "creates real CoreMIDI virtual ports; no physical controller needed"]
fn virtual_hotplug_feedback_and_headless_dispatch_latency() {
    use mixless_midi::midir::{
        os::unix::{VirtualInput, VirtualOutput},
        MidiInput, MidiOutput,
    };
    use std::{
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };
    let t = Temp::new();
    let h = ControllerManager::start_with_config(
        t.0.join("midi.json"),
        MidiConfig {
            enabled: true,
            input_ids: None,
        },
    )
    .unwrap();
    let samples = Arc::new(Mutex::new(Vec::new()));
    let received = samples.clone();
    let engine = Arc::new(
        mixless_engine::Engine::new(mixless_engine::EngineConfig {
            offline: true,
            ..Default::default()
        })
        .unwrap(),
    );
    let reader = engine.clone();
    h.set_handler(
        Arc::new(move |e| {
            if let Some(c) = e.action.command(&engine.snapshot()) {
                engine.dispatch(c).unwrap();
            }
            received.lock().unwrap().push(e);
            true
        }),
        Arc::new(move || reader.snapshot()),
    );
    let name = format!("mixless-independent-{}", new_id());
    let feedback = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let sink = feedback.clone();
    let _feedback_input = MidiInput::new("mixless-test-feedback")
        .unwrap()
        .create_virtual(
            &name,
            move |_, bytes, _| sink.lock().unwrap().push(bytes.to_vec()),
            (),
        )
        .unwrap();
    let mut source = MidiOutput::new("mixless-test-source")
        .unwrap()
        .create_virtual(&name)
        .unwrap();
    let wait = |condition: &dyn Fn() -> bool| {
        let until = Instant::now() + Duration::from_secs(3);
        while !condition() && Instant::now() < until {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(condition());
    };
    wait(&|| {
        h.devices()
            .iter()
            .any(|d| d.name == name && d.state == ConnectionState::Connected)
    });
    let id = h.devices().into_iter().find(|d| d.name == name).unwrap().id;
    let mut p = builtin_presets().remove(0);
    p.id = new_id();
    p.name = "Virtual fixture".into();
    p.feedback.clear();
    p.shutdown.clear();
    p.bindings.push(binding(MidiTarget::Master));
    let mut touch = binding(MidiTarget::JogTouch { deck: DeckId::A });
    touch.id = "touch".into();
    touch.source = InputSource::Midi {
        address: MidiAddress::Note {
            channel: 0,
            number: 5,
        },
    };
    p.bindings.push(touch);
    p.feedback.push(FeedbackRule {
        signal: FeedbackSignal::Control {
            target: MidiTarget::Master,
        },
        packet: OutputPacket {
            endpoint: "main".into(),
            data: vec![0x90, 12, 0],
            report: None,
        },
        bit_offset: 16,
        bits: 7,
        little_endian: true,
        off: 0,
        on: 127,
    });
    let preset = h.save_preset(p).unwrap();
    h.apply_preset_to(&preset, &id).unwrap();
    wait(&|| !feedback.lock().unwrap().is_empty());
    let count = feedback.lock().unwrap().len();
    thread::sleep(Duration::from_millis(120));
    assert_eq!(feedback.lock().unwrap().len(), count); // deduplicated feedback
                                                       // Never call drain or any UI tick while sending input for two seconds.
    for n in 0..500 {
        source.send(&[0xb2, 17, (n % 128) as u8]).unwrap();
        thread::sleep(Duration::from_millis(4));
    }
    wait(&|| samples.lock().unwrap().len() >= 500);
    assert!(h.drain().is_empty());
    let percentiles = h.latency_percentiles().unwrap();
    eprintln!(
        "CoreMIDI callback -> handler latency us: p50={} p95={} p99={}",
        percentiles[0], percentiles[1], percentiles[2]
    );
    assert!(percentiles[2] < 5000, "p99 exceeded 5 ms: {percentiles:?}");
    let name2 = format!("{}-second", name);
    let mut source2 = MidiOutput::new("mixless-test-second")
        .unwrap()
        .create_virtual(&name2)
        .unwrap();
    wait(&|| {
        h.devices()
            .iter()
            .any(|d| d.name == name2 && d.state == ConnectionState::Connected)
    });
    let id2 = h
        .devices()
        .into_iter()
        .find(|d| d.name == name2)
        .unwrap()
        .id;
    h.apply_preset_to(&preset, &id2).unwrap();
    thread::sleep(Duration::from_millis(40));
    h.learn_target_for(MidiTarget::Master, Some(id.clone()))
        .unwrap();
    let before = samples.lock().unwrap().len();
    source2.send(&[0xb2, 17, 44]).unwrap();
    wait(&|| samples.lock().unwrap().len() > before);
    assert!(h.learned().is_none());
    source.send(&[0xb2, 17, 45]).unwrap();
    wait(&|| h.learned().is_some());
    assert_eq!(h.learned().unwrap().device_id, id);
    h.set_learn(false);
    source.send(&[0x90, 5, 127]).unwrap();
    wait(&|| {
        samples.lock().unwrap().iter().any(|e| {
            matches!(e.action.target, MidiTarget::JogTouch { .. })
                && e.action.value == MidiValue::Press
        })
    });
    drop(source);
    wait(&|| {
        samples.lock().unwrap().iter().any(|e| {
            matches!(e.action.target, MidiTarget::JogTouch { .. })
                && e.action.value == MidiValue::Release
        })
    });
    wait(&|| {
        h.devices()
            .iter()
            .any(|d| d.id == id && d.state == ConnectionState::Offline)
    });
}
