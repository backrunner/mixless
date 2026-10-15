use super::*;
use crate::runtime::test_support::worker;

fn device(name: &str, vendor_id: u16, product_id: u16, page: u16, usage: u16) -> ControllerDevice {
    ControllerDevice {
        id: format!("hid:{vendor_id:04x}:{product_id:04x}:test:0:{page}:{usage}"),
        name: name.into(),
        hid: true,
        state: ConnectionState::Discovered,
        vendor_id: Some(vendor_id),
        product_id: Some(product_id),
        interface: Some(0),
        usage_page: Some(page),
        usage: Some(usage),
    }
}

fn preset() -> ControllerPreset {
    // Fictional, independently authored HID example; not a hardware claim.
    crate::profile::read_preset(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/declarative-hid/preset.json"),
    )
    .unwrap()
}

#[test]
fn hid_discovery_rejects_desktop_peripherals_and_unknown_vendor_usages() {
    let (_temp, mut w, _rx, _) = worker();
    w.shared.store.lock().unwrap().state.presets.push(preset());
    w.reload();
    for d in [
        device("Better75 USB", 0x2233, 0x0047, 1, 6),
        device("Better75 USB", 0x2233, 0x0047, 0xff00, 1),
        device("Magic Trackpad", 0x004c, 0x0265, 1, 2),
        device("CanoKey Canary", 0xffff, 1, 0xf1d0, 1),
        device("Headset", 0xffff, 2, 0x0c, 1),
        device("BTM", 0xffff, 3, 0xff00, 1),
        device("wakehint", 0xffff, 4, 0xff00, 1),
        device("DJ Controller", 0x1234, 0x5678, 0xff00, 1),
    ] {
        assert!(!w.recognized_hid(&d), "unrecognized {} passed", d.name);
    }
}

#[test]
fn unassigned_hid_preset_enables_only_its_declared_endpoint_and_removal_hides_it() {
    let (_temp, mut w, _rx, _) = worker();
    let d = device("Independent HID fixture", 0xffff, 0xffff, 0xff00, 1);
    w.reload();
    assert!(!w.recognized_hid(&d));
    let mut p = preset();
    if let DeviceMatcher::Hid {
        interface, usage, ..
    } = &mut p.endpoints[0].matcher
    {
        *interface = Some(0);
        *usage = Some(1);
    }
    w.shared.store.lock().unwrap().state.presets.push(p);
    w.shared.revision.fetch_add(1, Ordering::Release);
    w.reload();
    assert!(
        w.profiles.is_empty(),
        "discovery must not require assignment"
    );
    assert!(w.recognized_hid(&d));
    for wrong in [
        ControllerDevice {
            vendor_id: Some(0xfffe),
            ..d.clone()
        },
        ControllerDevice {
            product_id: Some(0xfffe),
            ..d.clone()
        },
        ControllerDevice {
            interface: Some(1),
            ..d.clone()
        },
        ControllerDevice {
            usage_page: Some(1),
            ..d.clone()
        },
        ControllerDevice {
            usage: Some(2),
            ..d.clone()
        },
        ControllerDevice {
            hid: false,
            ..d.clone()
        },
    ] {
        assert!(
            !w.recognized_hid(&wrong),
            "wrong endpoint passed: {wrong:?}"
        );
    }
    w.shared.store.lock().unwrap().state.presets.clear();
    w.shared.revision.fetch_add(1, Ordering::Release);
    w.reload();
    assert!(!w.recognized_hid(&d));
}

#[test]
fn filtered_hid_is_closed_and_does_not_reappear_as_a_stale_device() {
    let (_temp, mut w, _rx, _) = worker();
    *w.shared.config.lock().unwrap() = MidiConfig {
        enabled: false,
        input_ids: None,
    };
    let d = device("Better75 USB", 0x2233, 0x0047, 1, 6);
    w.shared.devices.lock().unwrap().push(d.clone());
    let (connection, _requests) = HidConnection::test_queue();
    w.hid.insert(d.id.clone(), connection);
    w.scan();
    assert!(!w.hid.contains_key(&d.id));
    assert!(
        !w.shared
            .devices
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.id == d.id)
    );
}

#[test]
#[cfg(target_os = "macos")]
fn native_hid_inventory_only_publishes_recognized_controllers_without_opening_devices() {
    let (_temp, mut w, _rx, _) = worker();
    *w.shared.config.lock().unwrap() = MidiConfig {
        enabled: false,
        input_ids: None,
    };
    w.hid_inventory = crate::hid_backend::devices().unwrap();
    w.scan();
    assert!(
        w.shared
            .devices
            .lock()
            .unwrap()
            .iter()
            .filter(|d| d.hid)
            .all(|d| w.recognized_hid(d))
    );
    assert!(w.hid.is_empty());
    assert!(w.hid_open.is_empty());
}
