use std::collections::HashSet;

use super::WifiState;
use super::backend::relevant_change;
use super::interface::{ACTIVATED, AP, DEVICE, NM, PATH, WIFI_DEVICE, WIRELESS};
use super::types::Snapshot;
use mecha_wayland::dbus::Value;
use mecha_wayland::prelude::*;

fn props(entries: &[(&str, Value)]) -> PropMap {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect()
}

#[test]
fn radio_and_connection_states() {
    let mut snapshot = Snapshot {
        valid: true,
        radio: Some(false),
        connected: true,
        ..Snapshot::default()
    };
    assert_eq!(snapshot.state(), Some(WifiState::Disabled));
    snapshot.radio = Some(true);
    snapshot.connected = false;
    assert_eq!(snapshot.state(), Some(WifiState::Disconnected));
    snapshot.connected = true;
    assert_eq!(snapshot.state(), Some(WifiState::None));
    for (strength, expected) in [
        (24, WifiState::None),
        (25, WifiState::Low),
        (49, WifiState::Low),
        (50, WifiState::Medium),
        (74, WifiState::Medium),
        (75, WifiState::High),
    ] {
        snapshot.strength = Some(strength);
        assert_eq!(snapshot.state(), Some(expected));
    }
    snapshot.strength = None;
    snapshot.record_strength(20);
    snapshot.record_strength(75);
    snapshot.record_strength(30);
    assert_eq!(snapshot.strength, Some(75));
}

#[test]
fn incomplete_scan_does_not_publish_disconnected() {
    let mut snapshot = Snapshot {
        valid: true,
        ..Snapshot::default()
    };
    assert_eq!(snapshot.state(), None);
    snapshot.radio = Some(true);
    snapshot.valid = false;
    assert_eq!(snapshot.state(), None);
}

#[test]
fn manager_requires_a_complete_radio_read() {
    for (enabled, hardware, expected) in [
        (true, true, Some(WifiState::Disconnected)),
        (false, true, Some(WifiState::Disabled)),
        (true, false, Some(WifiState::Disabled)),
        (false, false, Some(WifiState::Disabled)),
    ] {
        let mut snapshot = Snapshot {
            valid: true,
            ..Snapshot::default()
        };
        let radio = props(&[
            ("WirelessEnabled", Value::Bool(enabled)),
            ("WirelessHardwareEnabled", Value::Bool(hardware)),
        ]);
        assert_eq!(snapshot.manager(Some(&radio)), enabled && hardware);
        assert_eq!(snapshot.state(), expected);
    }
    for radio in [
        None,
        Some(props(&[("WirelessEnabled", Value::Bool(true))])),
        Some(props(&[("WirelessEnabled", Value::U32(1))])),
    ] {
        let mut snapshot = Snapshot {
            valid: true,
            ..Snapshot::default()
        };
        assert!(!snapshot.manager(radio.as_ref()));
        assert_eq!(snapshot.state(), None);
    }
}

#[test]
fn devices_only_follow_activated_wifi_and_preserve_unknown_scans() {
    let wired = ObjectPath::new("/org/freedesktop/NetworkManager/Devices/1").unwrap();
    let disconnected = ObjectPath::new("/org/freedesktop/NetworkManager/Devices/2").unwrap();
    let activated = ObjectPath::new("/org/freedesktop/NetworkManager/Devices/3").unwrap();
    let mut snapshot = Snapshot {
        radio: Some(true),
        valid: true,
        ..Snapshot::default()
    };
    assert!(!snapshot.device(
        &wired,
        Some(&props(&[
            ("DeviceType", Value::U32(1)),
            ("State", Value::U32(ACTIVATED)),
        ]))
    ));
    assert!(snapshot.connections.is_empty());
    assert!(!snapshot.device(
        &disconnected,
        Some(&props(&[
            ("DeviceType", Value::U32(WIFI_DEVICE)),
            ("State", Value::U32(30)),
        ]))
    ));
    assert_eq!(snapshot.state(), Some(WifiState::Disconnected));
    assert_eq!(snapshot.connections.len(), 1);
    assert_eq!(snapshot.connections[0].device, disconnected);
    assert!(snapshot.device(
        &activated,
        Some(&props(&[
            ("DeviceType", Value::U32(WIFI_DEVICE)),
            ("State", Value::U32(ACTIVATED)),
        ]))
    ));
    assert_eq!(snapshot.state(), Some(WifiState::None));
    assert_eq!(snapshot.connections.len(), 2);
    assert_eq!(snapshot.connections[1].device, activated);
    assert!(
        snapshot
            .connections
            .iter()
            .all(|device| device.connection.is_none())
    );
    assert!(!snapshot.device(&wired, None));
    assert_eq!(snapshot.connections.len(), 2);
    assert_eq!(snapshot.state(), None);

    for missing in [
        props(&[("State", Value::U32(ACTIVATED))]),
        props(&[("DeviceType", Value::U32(WIFI_DEVICE))]),
        props(&[("DeviceType", Value::Bool(true))]),
    ] {
        let mut snapshot = Snapshot {
            valid: true,
            radio: Some(true),
            ..Snapshot::default()
        };
        assert!(!snapshot.device(&disconnected, Some(&missing)));
        assert_eq!(snapshot.state(), None);
    }
}

#[test]
fn wireless_ap_reads_deduplicate_and_choose_strongest() {
    let mut snapshot = Snapshot {
        valid: true,
        radio: Some(true),
        connected: true,
        ..Snapshot::default()
    };
    let first = ObjectPath::new("/org/freedesktop/NetworkManager/AccessPoint/1").unwrap();
    let second = ObjectPath::new("/org/freedesktop/NetworkManager/AccessPoint/2").unwrap();
    let first_device = ObjectPath::new("/org/freedesktop/NetworkManager/Devices/1").unwrap();
    let second_device = ObjectPath::new("/org/freedesktop/NetworkManager/Devices/2").unwrap();
    let device_props = props(&[
        ("DeviceType", Value::U32(WIFI_DEVICE)),
        ("State", Value::U32(ACTIVATED)),
    ]);
    assert!(snapshot.device(&first_device, Some(&device_props)));
    assert!(snapshot.device(&second_device, Some(&device_props)));
    let no_ap = props(&[(
        "ActiveAccessPoint",
        Value::ObjectPath(ObjectPath::new("/").unwrap()),
    )]);
    assert_eq!(snapshot.wireless(&first_device, Some(&no_ap)), None);
    assert!(snapshot.connections[0].access_point.is_none());
    assert_eq!(snapshot.state(), Some(WifiState::None));
    let first_props = props(&[("ActiveAccessPoint", Value::ObjectPath(first.clone()))]);
    // Replies can arrive out of device order.
    assert_eq!(
        snapshot.wireless(&second_device, Some(&first_props)),
        Some(first.clone())
    );
    assert!(snapshot.connections[0].access_point.is_none());
    assert_eq!(snapshot.connections[1].access_point, Some(first.clone()));
    // Deduplicating the AP read must still record it on the other device.
    assert_eq!(snapshot.wireless(&first_device, Some(&first_props)), None);
    assert_eq!(snapshot.connections[0].access_point, Some(first.clone()));
    snapshot.access_point(
        &first,
        Some(&props(&[
            ("Strength", Value::U8(35)),
            ("Ssid", Value::from(b"Home".to_vec())),
        ])),
    );
    assert_eq!(snapshot.state(), Some(WifiState::Low));
    for wifi in &snapshot.connections {
        let connection = wifi.connection.as_ref().unwrap();
        assert_eq!(connection.strength, 35);
        assert_eq!(connection.ssid, b"Home");
    }
    let second_props = props(&[("ActiveAccessPoint", Value::ObjectPath(second.clone()))]);
    assert_eq!(
        snapshot.wireless(&second_device, Some(&second_props)),
        Some(second.clone())
    );
    assert_eq!(snapshot.connections[0].access_point, Some(first.clone()));
    assert_eq!(snapshot.connections[1].access_point, Some(second.clone()));
    snapshot.access_point(
        &second,
        Some(&props(&[
            ("Strength", Value::U8(80)),
            ("Ssid", Value::from("Café".as_bytes().to_vec())),
        ])),
    );
    assert_eq!(snapshot.state(), Some(WifiState::High));
    snapshot.access_point(&first, Some(&props(&[("Strength", Value::U8(15))])));
    assert_eq!(snapshot.state(), Some(WifiState::High));
    let unrelated = ObjectPath::new("/org/freedesktop/NetworkManager/AccessPoint/3").unwrap();
    snapshot.access_point(&unrelated, Some(&props(&[("Strength", Value::U8(100))])));
    assert_eq!(snapshot.strength, Some(80));
    let first_connection = snapshot.connections[0].connection.as_ref().unwrap();
    assert_eq!(first_connection.strength, 15);
    assert_eq!(first_connection.ssid, b"Home");
    assert_eq!(
        snapshot.connections[1].connection.as_ref().unwrap().strength,
        80
    );
    assert_eq!(
        snapshot.connections[1].connection.as_ref().unwrap().ssid,
        "Café".as_bytes()
    );
    assert_eq!(snapshot.wireless(&first_device, Some(&no_ap)), None);
    assert!(snapshot.connections[0].access_point.is_none());
    assert_eq!(snapshot.connections[1].access_point, Some(second));
}

#[test]
fn ssid_updates_preserve_empty_and_non_utf8_bytes_without_changing_strength() {
    let device = ObjectPath::new("/org/freedesktop/NetworkManager/Devices/1").unwrap();
    let ap = ObjectPath::new("/org/freedesktop/NetworkManager/AccessPoint/1").unwrap();
    let mut snapshot = Snapshot::default();
    snapshot.device(
        &device,
        Some(&props(&[
            ("DeviceType", Value::U32(WIFI_DEVICE)),
            ("State", Value::U32(ACTIVATED)),
        ])),
    );
    snapshot.wireless(
        &device,
        Some(&props(&[("ActiveAccessPoint", Value::ObjectPath(ap.clone()))])),
    );
    snapshot.access_point(&ap, Some(&props(&[("Strength", Value::U8(35))])));

    for bytes in [vec![], vec![b'A', 0xff]] {
        snapshot.access_point(&ap, Some(&props(&[("Ssid", Value::from(bytes.clone()))])));
        let connection = snapshot.connections[0].connection.as_ref().unwrap();
        assert_eq!(connection.ssid, bytes);
        assert_eq!(connection.strength, 35);
    }

    // A D-Bus string is not a valid SSID byte array.
    snapshot.access_point(
        &ap,
        Some(&props(&[("Ssid", Value::Str("wrong type".into()))])),
    );
    assert_eq!(
        snapshot.connections[0].connection.as_ref().unwrap().ssid,
        [b'A', 0xff]
    );
    assert_eq!(snapshot.strength, Some(35));
}

#[test]
fn missing_ap_properties_do_not_forge_a_connection_or_strength() {
    let device = ObjectPath::new("/org/freedesktop/NetworkManager/Devices/1").unwrap();
    for wireless in [None, Some(PropMap::new())] {
        let mut snapshot = Snapshot {
            valid: true,
            radio: Some(true),
            connected: true,
            ..Snapshot::default()
        };
        assert!(snapshot.device(
            &device,
            Some(&props(&[
                ("DeviceType", Value::U32(WIFI_DEVICE)),
                ("State", Value::U32(ACTIVATED)),
            ]))
        ));
        assert_eq!(snapshot.wireless(&device, wireless.as_ref()), None);
        assert!(snapshot.connections[0].access_point.is_none());
        assert_eq!(snapshot.state(), None);
    }
    let ap = ObjectPath::new("/org/freedesktop/NetworkManager/AccessPoint/1").unwrap();
    let mut snapshot = Snapshot {
        valid: true,
        radio: Some(true),
        connected: true,
        ..Snapshot::default()
    };
    snapshot.active_aps.insert(ap.clone());
    snapshot.access_point(&ap, Some(&PropMap::new()));
    assert_eq!(snapshot.state(), Some(WifiState::None));
    snapshot.access_point(&ap, None);
    assert_eq!(snapshot.state(), None);
}

#[test]
fn only_relevant_property_signals_trigger_refresh() {
    let manager = ObjectPath::new(PATH).unwrap();
    let ap = ObjectPath::new("/org/freedesktop/NetworkManager/AccessPoint/1").unwrap();
    let other = ObjectPath::new("/org/freedesktop/NetworkManager/AccessPoint/2").unwrap();
    let empty = HashSet::new();
    let active = HashSet::from([ap.clone()]);
    let changed = props(&[("Strength", Value::U8(50))]);
    assert!(relevant_change(
        AP,
        Some(&ap),
        &changed,
        &[],
        &active,
        &empty
    ));
    assert!(relevant_change(
        AP,
        Some(&ap),
        &changed,
        &[],
        &empty,
        &active
    ));
    assert!(!relevant_change(
        AP,
        Some(&other),
        &changed,
        &[],
        &active,
        &empty
    ));
    assert!(!relevant_change(AP, None, &changed, &[], &active, &empty));
    assert!(relevant_change(
        WIRELESS,
        Some(&other),
        &PropMap::new(),
        &["ActiveAccessPoint".into()],
        &empty,
        &empty,
    ));
    assert!(relevant_change(
        DEVICE,
        Some(&other),
        &props(&[("DeviceType", Value::U32(WIFI_DEVICE))]),
        &[],
        &empty,
        &empty,
    ));
    assert!(relevant_change(
        NM,
        Some(&manager),
        &props(&[("WirelessEnabled", Value::Bool(false))]),
        &[],
        &empty,
        &empty,
    ));
    assert!(!relevant_change(
        NM,
        Some(&other),
        &props(&[("WirelessEnabled", Value::Bool(false))]),
        &[],
        &empty,
        &empty,
    ));
    assert!(!relevant_change(
        DEVICE,
        Some(&other),
        &props(&[("Ip4Config", Value::U32(1))]),
        &[],
        &empty,
        &empty,
    ));

    let signal = Message::signal(
        ap.clone(),
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
    )
    .with_body(&(AP, PropMap::new(), vec!["Strength".to_string()]));
    let (interface, changed, invalidated) = PropertiesChanged::parse(&signal).unwrap();
    assert!(relevant_change(
        interface.as_str(),
        signal.path(),
        &changed,
        &invalidated,
        &active,
        &empty,
    ));
}
