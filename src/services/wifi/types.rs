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

/// The UI should reread the current `WifiStatus` snapshot.
pub struct WifiChanged;
impl Event for WifiChanged {}

/// Request a change to NetworkManager's software Wi-Fi switch.
pub struct SetWifiEnabled(pub bool);
impl Signal for SetWifiEnabled {}

pub(super) struct Matches {
    pub(super) properties: MatchId,
    pub(super) devices: MatchId,
    pub(super) owner: MatchId,
}

/// Latest observed Wi-Fi state, derived from NetworkManager's radio and devices.
pub struct WifiStatus {
    pub state: WifiState,
    pub(super) pending: HashMap<CallId, Request>,
    pub(crate) snapshot: Snapshot,
    pub(super) active_aps: HashSet<ObjectPath>,
    pub(super) dirty: bool,
    pub(super) matches: Matches,
}
impl Resource for WifiStatus {}

pub(super) enum Request {
    /// Set the manager's WirelessEnabled property.
    SetWirelessEnabled,
    /// Query org.freedesktop.NetworkManager directly
    Manager,
    /// call GetDevices()
    Devices,
    /// Request made to network device, NetworkManager.Device interface
    Device(ObjectPath),
    /// Read Device.Wireless properties for this device path.
    Wireless(ObjectPath),
    /// Get AccessPoint information
    AccessPoint(ObjectPath),
}

#[derive(Default)]
pub(crate) struct Snapshot {
    /// NetworkManager's software Wi-Fi switch; None means unknown.
    pub(crate) wireless_enabled: Option<bool>,
    /// Whether Wi-Fi is enabled by hardware; None means unknown.
    pub(crate) wireless_hardware_enabled: Option<bool>,
    pub(crate) connected: bool,
    pub(crate) strength: Option<u8>,
    pub(crate) connections: SmallVec<[WifiDevice; 4]>,
    pub(crate) active_aps: HashSet<ObjectPath>,
    pub(crate) valid: bool,
}

pub(crate) struct WifiDevice {
    pub(crate) device: ObjectPath,
    /// None if device isn't connected to anything yet
    pub(crate) access_point: Option<ObjectPath>,
    pub(crate) connection: Option<Connection>,
}

pub(crate) struct Connection {
    /// Raw SSID bytes, decoded for display by the UI.
    pub(crate) ssid: Vec<u8>,
    /// Strength of the connection
    pub(crate) strength: u8,
}
