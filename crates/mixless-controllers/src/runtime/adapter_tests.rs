use super::*;
use crate::adapter::tests::preset;
use crate::runtime::test_support::worker;
fn attach(w: &mut Worker, id: &str, p: ControllerPreset) {
    w.profiles.push((
        DeviceAssignment {
            device_id: id.into(),
            preset_id: p.id.clone(),
            ports: [("main".into(), id.into())].into(),
            outputs: Default::default(),
        },
        p.clone(),
    ));
    w.adapters
        .insert(id.into(), AdapterWorker::start(id.into(), p, w.tx.clone()));
}
fn receive(w: &mut Worker, rx: &mpsc::Receiver<Event>) {
    match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
        Event::Adapter(r) => w.adapter_reply(r),
        _ => panic!("unexpected event"),
    }
}

#[test]
fn one_adapter_fault_releases_its_hold_and_other_devices_keep_working() {
    let (_temp, mut w, rx, events) = worker();
    attach(&mut w,"bad",preset("(if (i32.eqz (local.get $kind)) (then (call $emit (i32.const 0) (i32.const 1) (f64.const 1))) (else (loop $forever (br $forever))))"));
    attach(
        &mut w,
        "good",
        preset("(call $emit (i32.const 1) (i32.const 3) (f64.const 0.25))"),
    );
    w.adapter_event("bad", EventKind::Connect, "", &[], Instant::now());
    receive(&mut w, &rx);
    w.adapter_event("bad", EventKind::Input, "main", &[1], Instant::now());
    receive(&mut w, &rx);
    assert!(w.shared.adapter_errors.lock().unwrap().contains_key("bad"));
    w.adapter_event("good", EventKind::Input, "main", &[1], Instant::now());
    receive(&mut w, &rx);
    let log = events.lock().unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].action.value, MidiValue::Hold);
    assert_eq!(log[1].action.value, MidiValue::Release);
    assert_eq!(log[2].device_id, "good");
}
#[test]
fn old_generation_cannot_dispatch_after_reassignment() {
    let (_temp, mut w, rx, events) = worker();
    let p = preset("(call $emit (i32.const 1) (i32.const 3) (f64.const 0.25))");
    attach(&mut w, "unit", p.clone());
    w.adapter_event("unit", EventKind::Input, "main", &[1], Instant::now());
    let old = match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
        Event::Adapter(r) => r,
        _ => panic!(),
    };
    w.stop_adapter("unit");
    w.adapters.insert(
        "unit".into(),
        AdapterWorker::start("unit".into(), p, w.tx.clone()),
    );
    w.adapter_reply(old);
    assert!(events.lock().unwrap().is_empty());
    w.adapter_event("unit", EventKind::Input, "main", &[1], Instant::now());
    receive(&mut w, &rx);
    assert_eq!(events.lock().unwrap().len(), 1);
}

#[test]
fn cue_targets_share_the_decks_preview_hold() {
    for adapter_deck in [DeckId::A, DeckId::B] {
        let (_temp, mut w, rx, events) = worker();
        let mut p = ControllerPreset::blank("Independent cue hold fixture");
        let mut binding = Binding::from_legacy(&MidiBinding {
            address: None,
            inverted: false,
            sensitivity: 1.,
            ticks_per_revolution: None,
            pickup: false,
            device_id: None,
            kind: MidiSourceKind::Note,
            channel: 0,
            number: 4,
            target: MidiTarget::Cue {
                deck: DeckId::A,
                index: 0,
            },
            mode: MidiControlMode::Auto,
        });
        binding.mode = ValueMode::Momentary;
        p.bindings.push(binding);
        let midi = |w: &mut Worker, down| {
            for action in w.router.route(
                "midi",
                "main",
                &p,
                false,
                &[0x90, 4, if down { 127 } else { 0 }],
                &Default::default(),
            ) {
                w.deliver("midi".into(), Instant::now(), action);
            }
        };
        midi(&mut w, true);
        let mut adapter = preset("(call $emit (i32.const 0) (if (result i32) (i32.eqz (local.get $kind)) (then (i32.const 1)) (else (i32.const 2))) (f64.const 1))");
        adapter.adapter.as_mut().unwrap().controls[0] =
            MidiTarget::TemporaryCue { deck: adapter_deck };
        attach(&mut w, "adapter", adapter);
        w.adapter_event("adapter", EventKind::Connect, "", &[], Instant::now());
        receive(&mut w, &rx);
        events.lock().unwrap().clear();
        midi(&mut w, false);
        assert_eq!(
            events.lock().unwrap().len(),
            usize::from(adapter_deck != DeckId::A),
            "releasing a hot cue must not end another held preview on the same deck"
        );
        w.adapter_event("adapter", EventKind::Input, "main", &[0], Instant::now());
        receive(&mut w, &rx);
        assert_eq!(
            events.lock().unwrap().last().unwrap().action.value,
            MidiValue::Release
        );
    }
}
#[test]
fn queue_overflow_cannot_leave_a_control_held() {
    let (_temp, mut w, rx, events) = worker();
    attach(
        &mut w,
        "unit",
        preset("(call $emit (i32.const 0) (i32.const 1) (f64.const 1))"),
    );
    w.adapter_event("unit", EventKind::Connect, "", &[], Instant::now());
    receive(&mut w, &rx);
    *w.adapters["unit"].fault.lock().unwrap() = Some("Adapter input queue overflow".into());
    w.poll_adapters();
    assert_eq!(
        events.lock().unwrap().last().unwrap().action.value,
        MidiValue::Release
    );
    assert!(w.adapter_held.is_empty());
}
#[test]
fn declarative_and_adapter_holds_share_release_ownership() {
    for adapter_first in [true, false] {
        for disconnect in [true, false] {
            let (_temp, mut w, rx, events) = worker();
            let target = MidiTarget::JogTouch {
                deck: mixless_protocol::DeckId::A,
            };
            let mut p = ControllerPreset::blank("MIDI hold");
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
            let midi = |w: &mut Worker, down: bool| {
                let actions = w.router.route(
                    "midi",
                    "main",
                    &p,
                    false,
                    &[0x90, 4, if down { 127 } else { 0 }],
                    &Default::default(),
                );
                for action in actions {
                    w.deliver("midi".into(), Instant::now(), action);
                }
            };
            midi(&mut w, true);
            attach(&mut w, "adapter", preset("(call $emit (i32.const 0) (if (result i32) (i32.eqz (local.get $kind)) (then (i32.const 1)) (else (i32.const 2))) (f64.const 1))"));
            w.adapter_event("adapter", EventKind::Connect, "", &[], Instant::now());
            receive(&mut w, &rx);
            events.lock().unwrap().clear();
            let release_adapter = |w: &mut Worker| {
                if disconnect {
                    w.fail_adapter("adapter", "test fault".into());
                } else {
                    w.adapter_event("adapter", EventKind::Input, "main", &[0], Instant::now());
                    receive(w, &rx);
                }
            };
            if adapter_first {
                release_adapter(&mut w);
            } else {
                midi(&mut w, false);
            }
            assert!(
                events.lock().unwrap().is_empty(),
                "another device still holds the target"
            );
            if adapter_first {
                midi(&mut w, false);
            } else {
                release_adapter(&mut w);
            }
            let log = events.lock().unwrap();
            assert_eq!(log.len(), 1);
            assert_eq!(
                log[0].action,
                MidiAction {
                    target,
                    value: MidiValue::Release
                }
            );
        }
    }
}

#[test]
fn reassignment_cancels_old_declarative_output_without_affecting_other_devices() {
    let (_temp, mut w, _rx, _events) = worker();
    let p = ControllerPreset::blank("Old preset");
    let a = DeviceAssignment {
        device_id: "unit".into(),
        preset_id: p.id.clone(),
        ports: [("main".into(), "unit".into())].into(),
        outputs: Default::default(),
    };
    w.profiles.push((a.clone(), p.clone()));
    let (connection, pending) = HidConnection::test_queue();
    let generation = connection.generation;
    w.hid.insert("unit".into(), connection);
    let packet = OutputPacket {
        endpoint: "main".into(),
        data: vec![1, 127],
        report: Some(ReportKind::Output),
    };
    assert!(w.send(&a, &packet));
    let old = match pending.recv().unwrap() {
        HidRequest::Write(_, context) => context,
        _ => panic!("expected queued output"),
    };
    let unaffected = w.output_context("other", false);
    let replacement = ControllerPreset::blank("Replacement");
    {
        let mut store = w.shared.store.lock().unwrap();
        store.state.presets.push(replacement.clone());
        store.state.assignments.push(DeviceAssignment {
            preset_id: replacement.id,
            ..a
        });
    }
    w.reload();
    let next = w.output_context("unit", false);
    assert!(!old.active());
    assert!(next.active());
    assert!(unaffected.active());
    w.hid_result(
        "unit".into(),
        generation,
        HidResult::Write {
            context: next.clone(),
            endpoint: "main".into(),
            result: Err("current output failed".into()),
        },
    );
    for result in [Ok(()), Err("retired output failed".into())] {
        w.hid_result(
            "unit".into(),
            generation,
            HidResult::Write {
                context: old.clone(),
                endpoint: "main".into(),
                result,
            },
        );
        assert_eq!(
            w.shared
                .output_errors
                .lock()
                .unwrap()
                .get("unit")
                .map(String::as_str),
            Some("current output failed")
        );
    }
    w.release("unit");
    assert!(!next.active());
    assert!(unaffected.active());
}

#[test]
fn disabling_closes_recorded_sessions_and_queue_loss_marks_them_incomplete() {
    let (_temp, mut w, rx, _events) = worker();
    let p = preset("");
    let trace = crate::replay::ReportTrace {
        version: 1,
        preset_digest: crate::community::content_digest(&p),
        model: p.name.clone(),
        firmware: "test".into(),
        os: "test".into(),
        truncated: false,
        frames: vec![],
    };
    attach(&mut w, "unit", p.clone());
    *w.shared.recording.lock().unwrap() = Some(recording::Recorder::new("unit".into(), trace));
    w.adapter_event("unit", EventKind::Connect, "", &[], Instant::now());
    receive(&mut w, &rx);
    w.close_all();
    let trace = w
        .shared
        .recording
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .trace
        .clone();
    assert_eq!(
        trace.frames.last().unwrap().event.kind,
        EventKind::Disconnect
    );
    crate::replay::replay(&p, &trace, false).unwrap();

    w.adapters.insert(
        "unit".into(),
        AdapterWorker::start("unit".into(), p, w.tx.clone()),
    );
    *w.adapters["unit"].fault.lock().unwrap() = Some("Adapter input queue overflow".into());
    w.poll_adapters();
    assert!(
        w.shared
            .recording
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .trace
            .truncated
    );
}
#[test]
fn pending_hid_discovery_does_not_block_control_dispatch() {
    let (_temp, mut w, rx, events) = worker();
    let (_pending_reply, pending) = mpsc::channel();
    w.hid_scan = Some(pending);
    attach(
        &mut w,
        "unit",
        preset("(call $emit (i32.const 1) (i32.const 3) (f64.const 0.25))"),
    );
    for _ in 0..3 {
        w.poll_hid_inventory();
        w.adapter_event("unit", EventKind::Input, "main", &[1], Instant::now());
        receive(&mut w, &rx);
    }
    assert!(w.hid_scan.is_some(), "discovery has not replied");
    assert_eq!(events.lock().unwrap().len(), 3);
}

#[test]
fn input_overflow_cannot_replay_stale_presses_after_releasing_holds() {
    let (_temp, mut w, _rx, events) = worker();
    let mut p = ControllerPreset::blank("MIDI hold");
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
        target: MidiTarget::JogTouch {
            deck: mixless_protocol::DeckId::A,
        },
        mode: MidiControlMode::Auto,
    }));
    w.profiles.push((
        DeviceAssignment {
            device_id: "unit".into(),
            preset_id: p.id.clone(),
            ports: [("main".into(), "unit".into())].into(),
            outputs: Default::default(),
        },
        p,
    ));
    let packet = |at, down| ReceivedPacket {
        device_id: "unit".into(),
        hid: false,
        data: vec![0x90, 4, if down { 127 } else { 0 }],
        received_at: at,
    };
    let stale = Instant::now();
    w.receive(packet(stale, true));
    w.recover_overflow();
    assert_eq!(
        events.lock().unwrap().last().unwrap().action.value,
        MidiValue::Release
    );
    events.lock().unwrap().clear();
    w.receive(packet(stale, true));
    assert!(events.lock().unwrap().is_empty());
    w.receive(packet(Instant::now(), true));
    w.receive(packet(Instant::now(), false));
    let log = events.lock().unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].action.value, MidiValue::Press);
    assert_eq!(log[1].action.value, MidiValue::Release);
}

#[test]
fn timers_wait_for_earlier_callbacks_that_may_replace_the_deadline() {
    for replacement in [true, false] {
        let (_temp, mut w, rx, _events) = worker();
        let p = if replacement {
            preset("(if (i32.eqz (local.get $kind)) (then (call $timer (i32.const 10)))) (if (i32.eq (local.get $kind) (i32.const 1)) (then (call $timer (i32.const 60000))))")
        } else {
            preset("(if (i32.eqz (local.get $kind)) (then (call $timer (i32.const 10))))")
        };
        let trace = crate::replay::ReportTrace {
            version: 1,
            preset_digest: crate::community::content_digest(&p),
            model: p.name.clone(),
            firmware: "test".into(),
            os: "test".into(),
            truncated: false,
            frames: vec![],
        };
        attach(&mut w, "unit", p.clone());
        *w.shared.recording.lock().unwrap() = Some(recording::Recorder::new("unit".into(), trace));
        w.adapter_event("unit", EventKind::Connect, "", &[], Instant::now());
        receive(&mut w, &rx);
        std::thread::sleep(Duration::from_millis(12));
        w.adapter_event("unit", EventKind::Input, "main", &[1], Instant::now());
        // The input result is queued, but the dispatcher has not accepted its replacement timer yet.
        w.poll_adapters();
        receive(&mut w, &rx);
        w.poll_adapters();
        if !replacement {
            receive(&mut w, &rx);
        }
        w.release("unit");
        let trace = w
            .shared
            .recording
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .trace
            .clone();
        assert_eq!(
            trace
                .frames
                .iter()
                .any(|f| f.event.kind == EventKind::Timer),
            !replacement,
            "timers must respect earlier callbacks without disabling unchanged deadlines"
        );
        crate::replay::replay(&p, &trace, false).unwrap();
    }
}
#[test]
#[ignore = "timing measurement: run without concurrent builds or tests"]
fn isolated_adapter_dispatch_latency() {
    let (_temp, mut w, rx, _events) = worker();
    attach(
        &mut w,
        "unit",
        preset("(call $emit (i32.const 1) (i32.const 3) (f64.const 0.25))"),
    );
    w.adapter_event("unit", EventKind::Connect, "", &[], Instant::now());
    receive(&mut w, &rx);
    w.shared.latencies.lock().unwrap().clear();
    for _ in 0..500 {
        w.adapter_event("unit", EventKind::Input, "main", &[1], Instant::now());
        receive(&mut w, &rx);
    }
    let mut samples = w.shared.latencies.lock().unwrap().clone();
    samples.sort_unstable();
    eprintln!(
        "Wasm receive-to-dispatch p50={}us p95={}us p99={}us",
        samples[250], samples[474], samples[494]
    );
    assert!(samples[494] < 5000);
}
