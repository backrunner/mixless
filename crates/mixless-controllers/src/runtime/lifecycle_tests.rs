use super::*;
use crate::runtime::test_support::worker;

fn midi_preset(target: MidiTarget) -> ControllerPreset {
    let mut p = ControllerPreset::blank("Independent MIDI fixture");
    p.bindings.push(Binding::from_legacy(&MidiBinding {
        device_id: None,
        kind: MidiSourceKind::Note,
        channel: 0,
        number: 4,
        target,
        mode: MidiControlMode::Auto,
    }));
    p
}
fn assignment(id: &str, p: &ControllerPreset) -> DeviceAssignment {
    DeviceAssignment {
        device_id: id.into(),
        preset_id: p.id.clone(),
        ports: [("main".into(), id.into())].into(),
        outputs: Default::default(),
    }
}
fn packet(id: &str, at: Instant) -> ReceivedPacket {
    ReceivedPacket {
        device_id: id.into(),
        hid: false,
        data: vec![0x90, 4, 127],
        received_at: at,
    }
}

#[test]
fn switching_presets_discards_only_retired_devices_queued_input() {
    let (_temp, mut w, _rx, events) = worker();
    let old = midi_preset(MidiTarget::JogTouch { deck: DeckId::A });
    let new = midi_preset(MidiTarget::Play { deck: DeckId::B });
    let other = midi_preset(MidiTarget::Play { deck: DeckId::A });
    let a = assignment("unit", &old);
    let b = assignment("other", &other);
    w.profiles = vec![(a.clone(), old), (b.clone(), other.clone())];
    let queued_at = Instant::now();
    w.receive(packet("unit", queued_at));
    {
        let mut store = w.shared.store.lock().unwrap();
        store.state.presets = vec![new.clone(), other];
        store.state.assignments = vec![
            DeviceAssignment {
                preset_id: new.id,
                ..a
            },
            b,
        ];
    }
    w.reload();
    assert_eq!(
        events.lock().unwrap().last().unwrap().action.value,
        MidiValue::Release
    );
    events.lock().unwrap().clear();
    w.receive(packet("unit", queued_at));
    assert!(
        events.lock().unwrap().is_empty(),
        "old input reached the replacement"
    );
    w.receive(packet("other", queued_at));
    assert_eq!(
        events.lock().unwrap().len(),
        1,
        "unrelated input was discarded"
    );
    w.receive(packet("unit", Instant::now()));
    assert_eq!(
        events.lock().unwrap().last().unwrap().action.target,
        MidiTarget::Play { deck: DeckId::B }
    );
}

#[test]
fn reapply_initializes_and_refreshes_every_logical_endpoint() {
    let (_temp, mut w, _rx, _events) = worker();
    let mut p = ControllerPreset::blank("Independent two-interface fixture");
    let matcher = DeviceMatcher::Hid {
        vendor_id: 0xffff,
        product_id: 0xffff,
        interface: None,
        usage_page: None,
        usage: None,
    };
    p.endpoints = vec![
        Endpoint {
            id: "main".into(),
            matcher: matcher.clone(),
        },
        Endpoint {
            id: "lights".into(),
            matcher,
        },
    ];
    p.initialize = vec![
        OutputPacket {
            endpoint: "main".into(),
            data: vec![1, 5],
            report: Some(ReportKind::Output),
        },
        OutputPacket {
            endpoint: "lights".into(),
            data: vec![2, 6],
            report: Some(ReportKind::Output),
        },
    ];
    p.feedback = p
        .initialize
        .iter()
        .map(|packet| FeedbackRule {
            signal: FeedbackSignal::Control {
                target: MidiTarget::Master,
            },
            packet: OutputPacket {
                data: vec![packet.data[0], 0],
                ..packet.clone()
            },
            bit_offset: 8,
            bits: 7,
            little_endian: true,
            off: 0,
            on: 127,
        })
        .collect();
    p.validate().unwrap();
    let mut a = assignment("unit", &p);
    a.ports.insert("lights".into(), "secondary".into());
    w.profiles.push((a, p.clone()));
    let (main, main_rx) = HidConnection::test_queue();
    let (lights, lights_rx) = HidConnection::test_queue();
    w.hid.insert("unit".into(), main);
    w.hid.insert("secondary".into(), lights);
    w.feedback();
    let retired = match lights_rx.recv().unwrap() {
        HidRequest::Write(_, context) => context,
        _ => panic!(),
    };
    main_rx.recv().unwrap();
    w.revision = w.shared.revision.load(Ordering::Acquire);
    w.restart("unit");
    assert!(!retired.active());
    w.feedback();
    let writes = |rx: &mpsc::Receiver<HidRequest>| {
        rx.try_iter()
            .map(|r| match r {
                HidRequest::Write(packet, context) => {
                    assert!(context.active());
                    packet.data
                }
                _ => panic!(),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(writes(&main_rx).len(), 2);
    let secondary = writes(&lights_rx);
    assert_eq!(
        secondary.len(),
        2,
        "secondary initialization or feedback was omitted"
    );
    assert_eq!(secondary[0], vec![2, 6]);
}

#[test]
fn queued_hid_feedback_does_not_clear_a_failure_before_completion() {
    let (_temp, mut w, _rx, _events) = worker();
    let p = ControllerPreset::blank("Output fixture");
    let a = assignment("unit", &p);
    let (c, pending) = HidConnection::test_queue();
    let generation = c.generation;
    w.hid.insert("unit".into(), c);
    w.shared
        .output_errors
        .lock()
        .unwrap()
        .insert("unit".into(), "failed output".into());
    assert!(w.send(
        &a,
        &OutputPacket {
            endpoint: "main".into(),
            data: vec![1, 5],
            report: Some(ReportKind::Output)
        }
    ));
    assert_eq!(
        w.shared
            .output_errors
            .lock()
            .unwrap()
            .get("unit")
            .map(String::as_str),
        Some("failed output")
    );
    let context = match pending.recv().unwrap() {
        HidRequest::Write(_, context) => context,
        _ => panic!(),
    };
    w.hid_result(
        "unit".into(),
        generation,
        HidResult::Write {
            context,
            endpoint: "main".into(),
            result: Ok(()),
        },
    );
    assert!(!w.shared.output_errors.lock().unwrap().contains_key("unit"));
}

#[test]
fn editing_legacy_wildcard_releases_its_hold_without_releasing_device_bindings() {
    let (_temp, mut w, _rx, events) = worker();
    let mut p = midi_preset(MidiTarget::Brake { deck: DeckId::A });
    p.bindings[0].source = InputSource::Midi {
        address: MidiAddress::Note {
            channel: 0,
            number: 5,
        },
    };
    let a = assignment("unit", &p);
    w.profiles.push((a.clone(), p.clone()));
    let old = MidiBinding {
        device_id: None,
        kind: MidiSourceKind::Note,
        channel: 0,
        number: 4,
        target: MidiTarget::JogTouch { deck: DeckId::A },
        mode: MidiControlMode::Auto,
    };
    w.wildcard.bindings.push(Binding::from_legacy(&old));
    let queued_at = Instant::now();
    let mut assigned_press = packet("unit", queued_at);
    assigned_press.data[1] = 5;
    w.receive(assigned_press);
    w.receive(packet("unit", queued_at));
    events.lock().unwrap().clear();
    {
        let mut store = w.shared.store.lock().unwrap();
        store.state.presets = vec![p];
        store.state.assignments = vec![a];
        store.state.wildcard = vec![MidiBinding {
            target: MidiTarget::Play { deck: DeckId::B },
            ..old
        }];
    }
    w.reload();
    let releases = events.lock().unwrap().clone();
    assert_eq!(
        releases.len(),
        1,
        "wildcard editing did not release its own hold"
    );
    assert_eq!(
        releases[0].action.target,
        MidiTarget::JogTouch { deck: DeckId::A }
    );
    assert!(w.router.is_held(&MidiTarget::Brake { deck: DeckId::A }));
    events.lock().unwrap().clear();
    w.receive(packet("unit", queued_at));
    assert!(
        events.lock().unwrap().is_empty(),
        "old wildcard press used the new target"
    );
    w.receive(packet("unit", Instant::now()));
    assert_eq!(
        events.lock().unwrap().last().unwrap().action.target,
        MidiTarget::Play { deck: DeckId::B }
    );
    let mut release = packet("unit", Instant::now());
    release.data = vec![0x90, 5, 0];
    w.receive(release);
    assert_eq!(
        events.lock().unwrap().last().unwrap().action.target,
        MidiTarget::Brake { deck: DeckId::A }
    );
    assert_eq!(
        events.lock().unwrap().last().unwrap().action.value,
        MidiValue::Release
    );
}
