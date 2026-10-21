use std::collections::{HashMap, HashSet};

use mecha_wayland::prelude::*;
use smallvec::SmallVec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WifiState {
    #[default]
    High,
    Medium,
    Low,
    None,
    Disabled,
    Disconnected,
}

impl WifiState {
    #[inline]
    pub fn is_visible(&self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

pub(super) struct Matches {
    pub(super) properties: MatchId,
    pub(super) devices: MatchId,
    pub(super) owner: MatchId,
}

/// Latest observed Wi-Fi state, derived from NetworkManager's radio and devices.
pub struct WifiStatus {
    pub state: WifiState,
    pub(super) pending: HashMap<CallId, Request>,
    pub(super) snapshot: Snapshot,
    pub(super) active_aps: HashSet<ObjectPath>,
    pub(super) dirty: bool,
    pub(super) matches: Matches,
}
impl Resource for WifiStatus {}

pub(super) enum Request {
    /// Query org.freedesktop.NetworkManager directly
    Manager,
    /// call GetDevices()
    Devices,
    /// Request made to network device, NetworkManager.Device interface
    Device(ObjectPath),
    /// Request made to wifi device, NetworkManager.Wireless interface
    /// Uses internal name established by Request::Device
    Wireless,
    /// Get AccessPoint information
    AccessPoint(ObjectPath),
}

#[derive(Default)]
pub(super) struct Snapshot {
    /// Is wifi enabled ?
    /// None means we haven't decided yet.
    pub(super) radio: Option<bool>,
    pub(super) connected: bool,
    pub(super) strength: Option<u8>,
    pub(super) connections: SmallVec<[WifiDevice; 4]>,
    pub(super) active_aps: HashSet<ObjectPath>,
    pub(super) valid: bool,
}

pub(super) struct WifiDevice {
    pub(super) device: ObjectPath,
    /// None until connection information is populated.
    pub(super) connection: Option<Connection>,
}

pub(super) struct Connection {
    /// Network manager's DBus path
    access_point: ObjectPath,
    /// ssid name
    ssid: String,
    /// Strength of the connection
    strength: u8,
}
