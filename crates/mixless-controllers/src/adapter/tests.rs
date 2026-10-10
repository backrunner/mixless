use super::*;
use crate::community::*;
use crate::{DeviceMatcher, Endpoint, Provenance};
use mixless_protocol::DeckId;

pub(crate) fn preset(body: &str) -> ControllerPreset {
    let module = wat::parse_str(format!(r#"(module
      (import "mixless" "emit" (func $emit (param i32 i32 f64)))
      (import "mixless" "send" (func $send (param i32 i32 i32 i32)))
      (import "mixless" "read_feature" (func $read (param i32 i32 i32)))
      (import "mixless" "timer" (func $timer (param i32)))
      (memory (export "memory") 1 64)
      (func (export "mixless_buffer") (result i32) (i32.const 1024))
      (func (export "mixless_event") (param $kind i32) (param $endpoint i32) (param $data i32) (param $len i32) (param $time i64) (result i32)
        {body} (i32.const 0)))"#)).unwrap();
    let mut p = ControllerPreset::blank("Independent sandbox fixture");
    p.endpoints = vec![Endpoint {
        id: "main".into(),
        matcher: DeviceMatcher::Hid {
            vendor_id: 0xffff,
            product_id: 0xffff,
            interface: Some(0),
            usage_page: Some(0xff00),
            usage: Some(1),
        },
    }];
    p.provenance = vec![Provenance {
        url: "https://github.com/backrunner/mixless".into(),
        revision: "synthetic-v1".into(),
        section: "Independently invented test messages".into(),
    }];
    p.community = Some(CommunityInfo {
        package_id: "test.independent".into(),
        package_version: "1.0.0".into(),
        minimum_host_version: env!("CARGO_PKG_VERSION").into(),
        author: "mixless tests".into(),
        license: "MPL-2.0".into(),
        license_text: include_str!("../../../../LICENSE").into(),
        source_url: "https://github.com/backrunner/mixless".into(),
        tested_on: vec![],
    });
    let mut adapter = AdapterDefinition {
        api_version: 1,
        wasm_base64: String::new(),
        blake3: String::new(),
        controls: vec![
            MidiTarget::JogTouch { deck: DeckId::A },
            MidiTarget::Master,
            MidiTarget::Play { deck: DeckId::A },
        ],
        reports: vec![
            ReportPermission {
                endpoint: "main".into(),
                access: ReportAccess::Output,
                report_id: 1,
                length: 2,
            },
            ReportPermission {
                endpoint: "main".into(),
                access: ReportAccess::FeatureRead,
                report_id: 3,
                length: 2,
            },
        ],
    };
    adapter.set_module(&module);
    p.adapter = Some(adapter);
    p
}
fn event(kind: EventKind, at_ms: u64) -> AdapterEvent {
    AdapterEvent {
        kind,
        endpoint: "main".into(),
        data: vec![1, 0],
        at_ms,
    }
}
fn state(p: &ControllerPreset) -> AdapterState {
    AdapterState::from_snapshot(p.adapter.as_ref().unwrap(), &EngineSnapshot::default())
}

#[test]
fn guest_trap_discards_all_effects_and_releases_previous_holds() {
    let p = preset(
        r#"(if (i32.eqz (local.get $kind))
        (then (call $emit (i32.const 0) (i32.const 1) (f64.const 1))))
        (if (i32.eq (local.get $kind) (i32.const 1))
        (then (call $emit (i32.const 1) (i32.const 3) (f64.const 0.25)) unreachable))"#,
    );
    let mut a = AdapterInstance::new(&p).unwrap();
    assert_eq!(
        a.process(&event(EventKind::Connect, 0), &state(&p))
            .unwrap()
            .len(),
        1
    );
    assert!(a.process(&event(EventKind::Input, 1), &state(&p)).is_err());
    assert_eq!(
        a.release_held(),
        vec![AdapterEffect::Action {
            action: MidiAction {
                target: MidiTarget::JogTouch { deck: DeckId::A },
                value: MidiValue::Release
            }
        }]
    );
    assert!(a
        .process(&event(EventKind::Input, 2), &state(&p))
        .unwrap_err()
        .to_string()
        .contains("stopped"));
}

#[test]
fn fuel_memory_capabilities_and_invalid_values_stop_only_the_instance() {
    for (body, message) in [
        ("(loop $forever (br $forever))", "fuel"),
        ("(drop (memory.grow (i32.const 40)))", "grow"),
        (
            "(call $emit (i32.const 99) (i32.const 0) (f64.const 1))",
            "control",
        ),
        (
            "(call $emit (i32.const 1) (i32.const 3) (f64.const nan))",
            "finite",
        ),
        (
            "(call $emit (i32.const 0) (i32.const 3) (f64.const 1))",
            "kind",
        ),
        (
            "(call $send (i32.const 0) (i32.const 0) (i32.const 65535) (i32.const 2))",
            "memory",
        ),
        (
            "(call $send (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 2))",
            "permitted",
        ),
        ("(call $timer (i32.const 0))", "timer"),
    ] {
        let p = preset(body);
        let mut a = AdapterInstance::new(&p).unwrap();
        let e = a
            .process(&event(EventKind::Connect, 0), &state(&p))
            .unwrap_err()
            .to_string();
        assert!(e.to_lowercase().contains(message), "{body}: {e}");
    }
    let p = preset("(call $emit (i32.const 1) (i32.const 3) (f64.const 0.5))");
    assert_eq!(
        AdapterInstance::new(&p)
            .unwrap()
            .process(&event(EventKind::Input, 0), &state(&p))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn report_permissions_timers_and_effect_rate_are_enforced() {
    let p = preset("(i32.store8 (i32.const 0) (i32.const 1)) (call $send (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 2)) (call $read (i32.const 0) (i32.const 3) (i32.const 2)) (call $timer (i32.const 10))");
    let mut a = AdapterInstance::new(&p).unwrap();
    let effects = a
        .process(&event(EventKind::Connect, 0), &state(&p))
        .unwrap();
    assert!(
        matches!(&effects[..], [AdapterEffect::Write { packet }, AdapterEffect::ReadFeature { report_id:3,.. }, AdapterEffect::Timer { delay_ms:10 }] if packet.data == [1,0])
    );
    for i in 1..128 {
        a.process(&event(EventKind::Input, i), &state(&p)).unwrap();
    }
    assert!(a
        .process(&event(EventKind::Input, 128), &state(&p))
        .unwrap_err()
        .to_string()
        .contains("rate"));
    let p = preset(&"(call $emit (i32.const 2) (i32.const 0) (f64.const 1)) ".repeat(65));
    assert!(AdapterInstance::new(&p)
        .unwrap()
        .process(&event(EventKind::Input, 0), &state(&p))
        .unwrap_err()
        .to_string()
        .contains("budget"));
}

#[test]
fn imports_start_functions_checksums_and_api_versions_are_rejected() {
    let mut p = preset("");
    for text in [
        r#"(module (import "wasi_snapshot_preview1" "fd_write" (func)) (memory (export "memory") 1))"#,
        r#"(module (func $start) (start $start) (memory (export "memory") 1))"#,
    ] {
        p.adapter
            .as_mut()
            .unwrap()
            .set_module(&wat::parse_str(text).unwrap());
        assert!(AdapterInstance::new(&p).is_err());
    }
    let mut p = preset("");
    p.adapter.as_mut().unwrap().blake3 = "0".repeat(64);
    assert!(p.validate().is_err());
    let mut p = preset("");
    p.adapter.as_mut().unwrap().api_version = 2;
    assert!(p.validate().is_err());
}

#[test]
fn independent_instances_keep_memory_separate() {
    let p = preset("(i32.store (i32.const 0) (i32.add (i32.load (i32.const 0)) (i32.const 1))) (call $emit (i32.const 1) (i32.const 3) (f64.div (f64.convert_i32_u (i32.load (i32.const 0))) (f64.const 10)))");
    let mut a = AdapterInstance::new(&p).unwrap();
    let mut b = AdapterInstance::new(&p).unwrap();
    let first = a.process(&event(EventKind::Input, 0), &state(&p)).unwrap();
    assert_ne!(
        first,
        a.process(&event(EventKind::Input, 1), &state(&p)).unwrap()
    );
    assert_eq!(
        first,
        b.process(&event(EventKind::Input, 0), &state(&p)).unwrap()
    );
}
