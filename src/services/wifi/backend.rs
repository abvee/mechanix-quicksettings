//! Read-only NetworkManager client on mecha-wayland's system D-Bus connection.
use std::collections::HashSet;

use super::interface::{AP, DEVICE, NM, PATH, WIRELESS};
use super::types::{Request, Snapshot};
use super::{WifiChanged, WifiState, WifiStatus};
use mecha_wayland::prelude::*;

fn get_all(app: &mut App, path: ObjectPath, interface: &'static str, request: Request) {
    let call = app
        .resource_mut::<DBus>()
        .system()
        .object(NM, path)
        .get_all(interface);
    app.resource_mut::<WifiStatus>()
        .pending
        .insert(call, request);
}

pub(super) fn request_snapshot(app: &mut App) {
    if !app.resource::<WifiStatus>().pending.is_empty() {
        app.resource_mut::<WifiStatus>().dirty = true;
        // TODO: why keep this ^ in the first place ?
        return;
    }
    {
        let mut wifi = app.resource_mut::<WifiStatus>();
        wifi.dirty = false;
        wifi.snapshot = Snapshot {
            valid: true,
            ..Snapshot::default()
        };
    }
    get_all(
        app,
        ObjectPath::new(PATH).expect("valid NetworkManager path"),
        NM,
        Request::Manager,
    );
}

pub(super) fn relevant_change(
    interface: &str,
    path: Option<&ObjectPath>,
    changed: &PropMap,
    invalidated: &[String],
    active_aps: &HashSet<ObjectPath>,
    scan_aps: &HashSet<ObjectPath>,
) -> bool {
    let keys: &[&str] = match interface {
        NM if path.is_some_and(|path| path.as_str() == PATH) => {
            &["WirelessEnabled", "WirelessHardwareEnabled"]
        }
        DEVICE => &["State", "DeviceType"],
        WIRELESS => &["ActiveAccessPoint"],
        AP if path.is_some_and(|path| active_aps.contains(path) || scan_aps.contains(path)) => {
            &["Strength"]
        }
        _ => &[],
    };
    keys.iter()
        .any(|key| changed.contains_key(*key) || invalidated.iter().any(|v| v == key))
}

fn publish_state(app: &mut App, state: WifiState) {
    app.resource_mut::<WifiStatus>().state = state;
    let root = app.root();
    let mut targets = vec![root];
    targets.extend(app.descendants(root));
    app.emit(WifiChanged, targets);
}

fn reply_props(event: &MethodReply, interface: &str) -> Option<PropMap> {
    event
        .result
        .as_ref()
        .map_err(|error| eprintln!("[wifi] {interface} GetAll failed: {error:?}"))
        .ok()
        .and_then(|message| {
            message
                .body::<(PropMap,)>()
                .map_err(|error| eprintln!("[wifi] failed to parse {interface} GetAll: {error:?}"))
                .ok()
        })
        .map(|(props,)| props)
}

pub(super) fn on_snapshot_reply(app: &mut App, event: &MethodReply) {
    let Some(request) = app.resource_mut::<WifiStatus>().pending.remove(&event.call) else {
        return;
    };

    match request {
        Request::Manager => {
            let props = reply_props(event, NM);
            if app
                .resource_mut::<WifiStatus>()
                .snapshot
                .manager(props.as_ref())
            {
                let call = app.resource_mut::<DBus>().system().object(NM, PATH).call(
                    NM,
                    "GetDevices",
                    &(),
                );
                app.resource_mut::<WifiStatus>()
                    .pending
                    .insert(call, Request::Devices);
            }
        }
        Request::Devices => {
            if let Some(paths) = event
                .result
                .as_ref()
                .map_err(|error| eprintln!("[wifi] GetDevices failed: {error:?}"))
                .ok()
                .and_then(|message| {
                    message
                        .body::<(Vec<ObjectPath>,)>()
                        .map_err(|error| eprintln!("[wifi] failed to parse GetDevices: {error:?}"))
                        .ok()
                })
                .map(|(paths,)| paths)
            {
                for path in paths {
                    get_all(app, path.clone(), DEVICE, Request::Device(path));
                }
            } else {
                app.resource_mut::<WifiStatus>().snapshot.valid = false;
            }
        }
        Request::Device(path) => {
            let props = reply_props(event, DEVICE);
            if app
                .resource_mut::<WifiStatus>()
                .snapshot
                .device(&path, props.as_ref())
            {
                // TODO: Request::Devices shouldn't be sent again until
                // NetworkManager has been restarted.
                get_all(app, path.clone(), WIRELESS, Request::Wireless(path));
            }
        }
        Request::Wireless(device) => {
            let props = reply_props(event, WIRELESS);
            if let Some(path) = app
                .resource_mut::<WifiStatus>()
                .snapshot
                .wireless(&device, props.as_ref())
            {
                get_all(app, path.clone(), AP, Request::AccessPoint(path));
            }
        }
        Request::AccessPoint(path) => {
            let props = reply_props(event, AP);
            app.resource_mut::<WifiStatus>()
                .snapshot
                .access_point(&path, props.as_ref());
        }
    }

    if app.resource::<WifiStatus>().pending.is_empty() {
        if app.resource::<WifiStatus>().dirty {
            request_snapshot(app);
        } else if let Some(state) = app.resource::<WifiStatus>().snapshot.state() {
            let aps = app.resource::<WifiStatus>().snapshot.active_aps.clone();
            app.resource_mut::<WifiStatus>().active_aps = aps;
            publish_state(app, state);
        } else {
            eprintln!("[wifi] incomplete NetworkManager snapshot; keeping previous state");
        }
    }
}

pub(super) fn on_signal(app: &mut App, event: &SignalReceived) {
    let wifi = app.resource::<WifiStatus>();
    let owner = event.matches.contains(&wifi.matches.owner);
    let devices = event.matches.contains(&wifi.matches.devices);
    let properties = event.matches.contains(&wifi.matches.properties);

    if owner {
        if let Ok((_, _, new_owner)) = event.msg.body::<(String, String, String)>() {
            let mut wifi = app.resource_mut::<WifiStatus>();
            wifi.pending.clear();
            wifi.snapshot = Snapshot::default();
            wifi.active_aps.clear();
            wifi.dirty = false;
            publish_state(app, WifiState::Disabled);
            if !new_owner.is_empty() {
                request_snapshot(app);
            }
        }
    } else if devices
        && matches!(
            event.msg.member().map(|member| member.as_str()),
            Some("DeviceAdded" | "DeviceRemoved")
        )
    {
        // TODO: don't request a snapshot, just remove that device's data all
        // data from the resource if DeviceRemoved
        // If device added, queue up Request::Device(path)
        request_snapshot(app);
    } else if properties {
        if let Ok((interface, changed, invalidated)) = PropertiesChanged::parse(&event.msg) {
            if relevant_change(
                interface.as_str(),
                event.msg.path(),
                &changed,
                &invalidated,
                &wifi.active_aps,
                &wifi.snapshot.active_aps,
            ) {
                // TODO: better handling required here instead of requesting
                // snapshots all over again
                request_snapshot(app);
            }
        }
    }
}

pub(super) fn on_disconnect(app: &mut App, event: &BusDisconnected) {
    if event.bus == BusId::System {
        eprintln!("[wifi] system D-Bus disconnected; resetting state");
        let mut wifi = app.resource_mut::<WifiStatus>();
        wifi.pending.clear();
        wifi.snapshot = Snapshot::default();
        wifi.active_aps.clear();
        wifi.dirty = false;
        publish_state(app, WifiState::Disabled);
    }
}
