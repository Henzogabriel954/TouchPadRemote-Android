use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use touchpad_protocol::{HostOs, MouseButton, PairingStatus, PrivilegeMode, ScreenEdge};
use touchpad_storage::{
    AndroidConfig, EdgeLink, SavedComputer, DEFAULT_EDGE_COOLDOWN_MS, DEFAULT_EDGE_OVERSHOOT_PX,
    DEFAULT_HOLD_DURATION_MS, DEFAULT_LOCAL_SENSITIVITY,
};

/// Live host capability metadata reported by a desktop server via `HOST_INFO`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfoSnapshot {
    pub os: HostOs,
    pub width: u32,
    pub height: u32,
    pub privilege: PrivilegeMode,
}

/// Computer discovered on the local network via UDP broadcast `DISCOVER_RESP`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredComputer {
    pub name: String,
    pub ip_address: String,
    pub port: u16,
    pub connection_method_id: String,
}

/// Client-side Android configuration settings exposed to the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AndroidSettingsDto {
    pub multi_pc_enabled: bool,
    pub hold_duration_ms: u64,
    pub haptics_enabled: bool,
    pub default_landscape: bool,
    pub edge_overshoot_px: u16,
    pub local_sensitivity: f32,
}

impl Default for AndroidSettingsDto {
    fn default() -> Self {
        Self {
            multi_pc_enabled: false,
            hold_duration_ms: DEFAULT_HOLD_DURATION_MS,
            haptics_enabled: true,
            default_landscape: true,
            edge_overshoot_px: DEFAULT_EDGE_OVERSHOOT_PX,
            local_sensitivity: DEFAULT_LOCAL_SENSITIVITY,
        }
    }
}

impl From<&AndroidConfig> for AndroidSettingsDto {
    fn from(cfg: &AndroidConfig) -> Self {
        Self {
            multi_pc_enabled: cfg.multi_pc_enabled,
            hold_duration_ms: cfg.hold_duration_ms,
            haptics_enabled: cfg.haptics_enabled,
            default_landscape: cfg.default_landscape,
            edge_overshoot_px: cfg.edge_overshoot_px,
            local_sensitivity: cfg.local_sensitivity,
        }
    }
}

/// Partial or complete settings update payload accepted by `save_settings_json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AndroidSettingsPatch {
    #[serde(alias = "multiPcEnabled")]
    pub multi_pc_enabled: Option<bool>,
    #[serde(alias = "holdDurationMs")]
    pub hold_duration_ms: Option<u64>,
    #[serde(alias = "hapticsEnabled")]
    pub haptics_enabled: Option<bool>,
    #[serde(alias = "defaultLandscape")]
    pub default_landscape: Option<bool>,
    #[serde(alias = "edgeOvershootPx")]
    pub edge_overshoot_px: Option<u16>,
    #[serde(alias = "localSensitivity")]
    pub local_sensitivity: Option<f32>,
}

/// Input payload for adding or updating a saved computer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComputerInput {
    pub id: Option<String>,
    pub name: String,
    #[serde(alias = "ipAddress")]
    pub ip_address: String,
    pub port: Option<u16>,
    #[serde(alias = "connectionMethodId")]
    pub connection_method_id: Option<String>,
    #[serde(alias = "lastOs")]
    pub last_os: Option<String>,
    #[serde(alias = "lastPrivilege")]
    pub last_privilege: Option<String>,
}

/// Runtime status of a single PC participating in an active or connecting session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPeerSnapshot {
    pub computer_id: String,
    pub name: String,
    pub ip_address: String,
    pub port: u16,
    pub online: bool,
    pub pairing_status: Option<PairingStatus>,
    pub active_edges: Vec<ScreenEdge>,
}

/// Active single-PC or Multi-PC session state exposed to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveSessionSnapshot {
    pub primary_computer_id: String,
    pub active_computer_id: String,
    pub active_computer_name: String,
    pub multi_pc_enabled: bool,
    pub edge_locked: bool,
    pub connected: bool,
    pub peers: Vec<SessionPeerSnapshot>,
}

/// Complete immutable UI state snapshot serialized to JSON for Kotlin `MainViewModel`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiStateSnapshot {
    pub client_id: String,
    pub device_name: String,
    pub computers: Vec<SavedComputer>,
    pub online_status: HashMap<String, bool>,
    pub host_info_map: HashMap<String, HostInfoSnapshot>,
    pub discovered_computers: Vec<DiscoveredComputer>,
    pub is_scanning_network: bool,
    pub has_scanned_network: bool,
    pub edge_links: Vec<EdgeLink>,
    pub settings: AndroidSettingsDto,
    pub active_session: Option<ActiveSessionSnapshot>,
    pub checking_computer_id: Option<String>,
    pub pairing_computer_id: Option<String>,
}

/// Discrete event delivered to Kotlin via the `Condvar`-backed event queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UiEvent {
    StateUpdated,
    ActiveComputerChanged {
        computer_id: String,
        computer_name: String,
        via_edge: Option<ScreenEdge>,
    },
    PairingPending {
        computer_id: String,
        computer_name: String,
    },
    PairingRejected {
        computer_id: String,
        computer_name: String,
    },
    ComputerOffline {
        computer_id: String,
        computer_name: String,
    },
    SessionDisconnected {
        reason: String,
    },
    ToastMessage {
        message: String,
    },
}

/// Configurable engine intervals for production defaults and deterministic testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineTimings {
    pub initial_pair_timeout: Duration,
    pub pair_poll_interval: Duration,
    pub heartbeat_interval: Duration,
    pub refresh_timeout: Duration,
    pub edge_cooldown: Duration,
    pub socket_poll_interval: Duration,
    pub worker_tick_interval: Duration,
}

impl Default for EngineTimings {
    fn default() -> Self {
        Self {
            initial_pair_timeout: Duration::from_millis(800),
            pair_poll_interval: Duration::from_millis(900),
            heartbeat_interval: Duration::from_millis(2500),
            refresh_timeout: Duration::from_millis(600),
            edge_cooldown: Duration::from_millis(DEFAULT_EDGE_COOLDOWN_MS),
            socket_poll_interval: Duration::from_millis(50),
            worker_tick_interval: Duration::from_millis(50),
        }
    }
}

/// Maps a Kotlin/Java `MouseButton.ordinal` integer (`0 = Left`, `1 = Right`, `2 = Middle`).
pub fn mouse_button_from_ordinal(ordinal: i32) -> Option<MouseButton> {
    match ordinal {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Right),
        2 => Some(MouseButton::Middle),
        _ => None,
    }
}

/// Canonical uppercase string representation of [`HostOs`] for `SavedComputer.last_os`.
pub fn host_os_to_str(os: HostOs) -> &'static str {
    match os {
        HostOs::Windows => "WINDOWS",
        HostOs::Linux => "LINUX",
    }
}

/// Canonical uppercase string representation of [`PrivilegeMode`] for `SavedComputer.last_privilege`.
pub fn privilege_mode_to_str(privilege: PrivilegeMode) -> &'static str {
    match privilege {
        PrivilegeMode::User => "USER",
        PrivilegeMode::Admin => "ADMIN",
        PrivilegeMode::System => "SYSTEM",
        PrivilegeMode::Uinput => "UINPUT",
        PrivilegeMode::Xtest => "XTEST",
    }
}
