use super::*;
use crate::adapter::tests::preset;

fn frame(kind: EventKind, at_ms: u64, expected: Vec<AdapterEffect>) -> ReplayFrame {
    ReplayFrame {
        event: AdapterEvent {
            kind,
            endpoint: "main".into(),
            data: vec![1, 0],
            at_ms,
        },
        state: None,
        expected: Some(expected),
        expected_error: None,
    }
}
fn trace(p: &ControllerPreset, frames: Vec<ReplayFrame>) -> ReportTrace {
    ReportTrace {
        version: 1,
        preset_digest: content_digest(p),
        model: "synthetic".into(),
        firmware: "synthetic-v1".into(),
        os: "offline".into(),
        truncated: false,
        frames,
    }
}
#[test]
fn replay_assertions_require_pinned_complete_ordered_sessions() {
    let p = preset("");
    let good = trace(
        &p,
        vec![
            frame(EventKind::Connect, 0, vec![]),
            frame(EventKind::Disconnect, 100, vec![]),
        ],
    );
    assert_eq!(replay(&p, &good, true).unwrap().assertions, 2);
    let mut bad = good.clone();
    bad.frames[0].expected = None;
    assert!(replay(&p, &bad, true).is_err());
    assert_eq!(replay(&p, &bad, false).unwrap().assertions, 1);
    let mut bad = good.clone();
    bad.preset_digest = "wrong".into();
    assert!(replay(&p, &bad, false).is_err());
    let mut bad = good.clone();
    bad.truncated = true;
    assert!(replay(&p, &bad, false).is_err());
    let mut bad = good.clone();
    bad.frames.pop();
    assert!(replay(&p, &bad, false).is_err());
    let mut bad = good.clone();
    bad.frames[0].event.at_ms = 101;
    assert!(replay(&p, &bad, false).is_err());
    let mut bad = good;
    bad.frames[0].event.kind = EventKind::Input;
    assert!(replay(&p, &bad, false).is_err());
}
#[test]
fn replay_checks_timers_traps_and_host_releases() {
    let p = preset("(if (i32.eqz (local.get $kind)) (then (call $timer (i32.const 100)) (call $emit (i32.const 0) (i32.const 1) (f64.const 1)))) (if (i32.eq (local.get $kind) (i32.const 3)) (then unreachable))");
    let action = |value| AdapterEffect::Action {
        action: MidiAction {
            target: MidiTarget::JogTouch {
                deck: mixless_protocol::DeckId::A,
            },
            value,
        },
    };
    let mut frames = vec![
        frame(
            EventKind::Connect,
            0,
            vec![
                AdapterEffect::Timer { delay_ms: 100 },
                action(MidiValue::Hold),
            ],
        ),
        frame(EventKind::Timer, 100, vec![action(MidiValue::Release)]),
        frame(EventKind::Disconnect, 101, vec![]),
    ];
    frames[1].expected_error = Some("unreachable".into());
    let mut t = trace(&p, frames);
    assert_eq!(replay(&p, &t, true).unwrap().assertions, 3);
    t.frames[1].event.at_ms = 99;
    assert!(replay(&p, &t, true).is_err());
}
#[test]
fn recorded_snapshots_exclude_library_and_device_identifiers() {
    let mut s = EngineSnapshot::default();
    s.decks[0].title = Some("Private set".into());
    s.decks[0].artist = Some("Personal library".into());
    s.device_name = "Private output".into();
    s.cue_device = Some("Local UID".into());
    let text = serde_json::to_string(&redact_snapshot(s)).unwrap();
    for private in [
        "Private set",
        "Personal library",
        "Private output",
        "Local UID",
    ] {
        assert!(!text.contains(private));
    }
}
#[test]
fn community_imports_are_separate_pinned_copies_with_atomic_failure_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("midi.json");
    let hub = ControllerManager::start_with_config(
        path.clone(),
        MidiConfig {
            enabled: false,
            input_ids: None,
        },
    )
    .unwrap();
    let mut p = preset("");
    p.verification = Verification::Hardware;
    let source = dir.path().join("package.json");
    fs::write(&source, serde_json::to_vec(&p).unwrap()).unwrap();
    let preview = hub.preview_import(&source).unwrap();
    assert!(hub.assignments().is_empty());
    assert!(!hub.presets().iter().any(|v| v.name == p.name));
    let id = hub.accept_import(preview).unwrap();
    let imported = hub.preset(&id).unwrap();
    assert_eq!(imported.verification, Verification::Protocol);
    assert_eq!(content_digest(&imported), content_digest(&p));
    let again = hub
        .accept_import(hub.preview_import(&source).unwrap())
        .unwrap();
    assert_ne!(id, again);
    let before = fs::read(hub.storage_path()).unwrap();
    let mut bad = p.clone();
    bad.adapter.as_mut().unwrap().api_version = 2;
    fs::write(&source, serde_json::to_vec(&bad).unwrap()).unwrap();
    assert!(hub.preview_import(&source).is_err());
    assert_eq!(before, fs::read(hub.storage_path()).unwrap());
    let export = dir.path().join("export.json");
    hub.export_preset(&id, &export).unwrap();
    assert_eq!(
        content_digest(&read_preset(&export).unwrap()),
        content_digest(&p)
    );
    assert!(!fs::read_to_string(&export).unwrap().contains("device_id"));
    drop(hub);
    let hub = ControllerManager::start_with_config(
        path,
        MidiConfig {
            enabled: false,
            input_ids: None,
        },
    )
    .unwrap();
    assert_eq!(hub.preset(&id).unwrap(), imported);
    fs::remove_file(hub.storage_path()).unwrap();
    fs::create_dir(hub.storage_path()).unwrap();
    assert!(hub.rename_preset(&id, "Changed").is_err());
    assert_eq!(hub.preset(&id).unwrap(), imported);
}
#[test]
fn future_host_and_unbounded_documents_are_rejected() {
    let mut p = preset("");
    p.community.as_mut().unwrap().minimum_host_version = "999.0.0".into();
    assert!(p.validate().is_err());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("huge.json");
    fs::write(&file, vec![b' '; 4 * 1024 * 1024 + 1]).unwrap();
    assert!(read_preset(&file).is_err());
}

#[test]
fn community_protocol_sources_cannot_be_blank_placeholders() {
    for field in ["url", "revision", "section"] {
        let mut p = preset("");
        let source = &mut p.provenance[0];
        match field {
            "url" => source.url = " ".into(),
            "revision" => source.revision.clear(),
            _ => source.section = "\n".into(),
        }
        assert!(
            p.validate().is_err(),
            "blank {field} bypassed provenance validation"
        );
    }
}

#[test]
fn independently_authored_declarative_example_routes_and_deduplicates_feedback() {
    let p: ControllerPreset =
        serde_json::from_str(include_str!("../../examples/declarative-hid/preset.json")).unwrap();
    let mut t: ReportTrace =
        serde_json::from_str(include_str!("../../examples/declarative-hid/fixture.json")).unwrap();
    t.preset_digest = content_digest(&p);
    assert_eq!(replay(&p, &t, true).unwrap().assertions, 6);
}

#[test]
fn declarative_replay_failure_releases_holds_before_reconnect() {
    let mut p = ControllerPreset::blank("Independent hold fixture");
    let target = MidiTarget::JogTouch { deck: DeckId::A };
    p.bindings.push(Binding::from_legacy(&MidiBinding {
        address: None,
        inverted: false,
        sensitivity: 1.,
        ticks_per_revolution: None,
        pickup: false,
        device_id: None,
        kind: MidiSourceKind::Note,
        channel: 0,
        number: 4,
        target: target.clone(),
        mode: MidiControlMode::Auto,
    }));
    let action = |value| AdapterEffect::Action {
        action: MidiAction {
            target: target.clone(),
            value,
        },
    };
    let mut press = frame(EventKind::Input, 1, vec![action(MidiValue::Press)]);
    press.event.data = vec![0x90, 4, 127];
    let mut invalid = frame(EventKind::Input, 2, vec![action(MidiValue::Release)]);
    invalid.event.endpoint = "missing".into();
    invalid.expected_error = Some("Unknown endpoint".into());
    let mut again = press.clone();
    again.event.at_ms = 5;
    let t = trace(
        &p,
        vec![
            frame(EventKind::Connect, 0, vec![]),
            press,
            invalid,
            frame(EventKind::Disconnect, 3, vec![]),
            frame(EventKind::Connect, 4, vec![]),
            again,
            frame(EventKind::Disconnect, 6, vec![action(MidiValue::Release)]),
        ],
    );
    assert_eq!(replay(&p, &t, true).unwrap().assertions, 7);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "records temporary CoreMIDI ports; no physical hardware"]
fn virtual_midi_capture_round_trip_excludes_local_identity() {
    use mixless_midi::midir::{os::unix::VirtualOutput, MidiOutput};
    use std::time::{Duration, Instant};
    let name = format!("mixless capture test {}", std::process::id());
    let mut output = MidiOutput::new(&name)
        .unwrap()
        .create_virtual(&name)
        .unwrap();
    let port = input_ports()
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let hub = ControllerManager::start_with_config(
        temp.path().join("midi.json"),
        MidiConfig {
            enabled: false,
            input_ids: None,
        },
    )
    .unwrap();
    let mut p = ControllerPreset::blank("Independent capture fixture");
    p.bindings.push(Binding::from_legacy(&MidiBinding {
        address: None,
        inverted: false,
        sensitivity: 1.,
        ticks_per_revolution: None,
        pickup: false,
        device_id: None,
        kind: MidiSourceKind::Cc,
        channel: 0,
        number: 7,
        target: MidiTarget::Master,
        mode: MidiControlMode::Absolute,
    }));
    let id = hub.save_preset(p.clone()).unwrap();
    hub.start_recording(&port.id, &id, "virtual-1", "macOS")
        .unwrap();
    hub.apply_preset_to(&id, &port.id).unwrap();
    hub.configure(MidiConfig {
        enabled: true,
        input_ids: Some(vec![port.id.clone()]),
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !hub
        .devices()
        .iter()
        .any(|d| d.id == port.id && d.state == ConnectionState::Connected)
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    output.send(&[0xb0, 7, 100]).unwrap();
    while hub.last_packet().is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    hub.unassign(&port.id).unwrap();
    let trace = hub.stop_recording().unwrap();
    let result = replay(&p, &trace, false).unwrap();
    assert!(result.outputs.iter().flatten().any(
        |e| matches!(e,AdapterEffect::Action {action} if action.value == MidiValue::Absolute(100))
    ));
    let encoded = serde_json::to_string(&trace).unwrap();
    assert!(!encoded.contains(&port.id));
    assert!(!encoded.contains(&name));
}
