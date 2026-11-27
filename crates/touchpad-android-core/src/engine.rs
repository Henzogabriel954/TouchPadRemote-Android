use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use touchpad_protocol::{
    ConnectionMethod, MouseButton, PairingStatus, TouchpadPacket, DEFAULT_PORT,
};
use touchpad_storage::{android_config_path, AndroidConfig, ConfigStore, EdgeLink, SavedComputer};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::discovery::discover_computers_on_lan;
use crate::error::AndroidEngineError;
use crate::events::EventBus;
use crate::graph::{find_edge_target, resolve_session_group, EdgeFlags};
use crate::models::{
    host_os_to_str, privilege_mode_to_str, AndroidSettingsDto, AndroidSettingsPatch,
    ComputerInput, DiscoveredComputer, EngineTimings, HostInfoSnapshot, UiEvent, UiStateSnapshot,
};
use crate::session::{socket_addrs_match, MultiPcSession, SessionPeer, SessionPhase};

const RECEIVE_BUFFER_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy)]
struct HotTarget {
    addr: SocketAddr,
    local_sensitivity: f32,
}

#[derive(Debug, Clone)]
struct PendingRefresh {
    pending_ids: HashSet<String>,
    timestamp: i64,
    deadline: Instant,
}

#[derive(Debug, Default)]
struct RuntimeState {
    online_status: HashMap<String, bool>,
    host_info_map: HashMap<String, HostInfoSnapshot>,
    discovered_computers: Vec<DiscoveredComputer>,
    is_scanning_network: bool,
    has_scanned_network: bool,
    checking_computer_id: Option<String>,
    pairing_computer_id: Option<String>,
    session: Option<MultiPcSession>,
    endpoint_cache: HashMap<String, SocketAddr>,
    pending_refresh: Option<PendingRefresh>,
    discovery_cancel: Option<Arc<AtomicBool>>,
}

#[derive(Default)]
struct ThreadHandles {
    reader: Option<JoinHandle<()>>,
    worker: Option<JoinHandle<()>>,
    discovery: Option<JoinHandle<()>>,
}

struct EngineShared {
    store: ConfigStore<AndroidConfig>,
    device_name: String,
    timings: EngineTimings,
    socket: UdpSocket,
    hot_target: RwLock<Option<HotTarget>>,
    session_active: AtomicBool,
    events: EventBus,
    state: Mutex<RuntimeState>,
    worker_wake_lock: Mutex<u64>,
    worker_wake: Condvar,
    shutdown_flag: AtomicBool,
}

/// Core pure-Rust Android client engine managing persistent configuration, LAN discovery,
/// a single bidirectional UDP socket, Multi-PC session routing, and a `Condvar` event queue.
pub struct AndroidEngine {
    shared: Arc<EngineShared>,
    threads: Mutex<ThreadHandles>,
}

impl AndroidEngine {
    /// Initializes the engine using `android_config_path(&files_dir)` and production timings.
    pub fn new(files_dir: PathBuf, device_name: String) -> Result<Self, AndroidEngineError> {
        Self::with_timings(files_dir, device_name, EngineTimings::default())
    }

    /// Initializes the engine with custom [`EngineTimings`] (useful for fast deterministic tests).
    pub fn with_timings(
        files_dir: PathBuf,
        device_name: String,
        timings: EngineTimings,
    ) -> Result<Self, AndroidEngineError> {
        let config_path = android_config_path(&files_dir);
        let store = ConfigStore::<AndroidConfig>::new(config_path)?;
        let sanitized_device_name = sanitize_device_name(&device_name);

        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(timings.socket_poll_interval))?;

        let mut initial_state = RuntimeState::default();
        for comp in &store.get().computers {
            if let Ok(ip) = comp.ip_address.trim().parse::<IpAddr>() {
                let port = if comp.port > 0 { comp.port } else { DEFAULT_PORT };
                initial_state
                    .endpoint_cache
                    .insert(comp.id.clone(), SocketAddr::new(ip, port));
            }
        }

        let shared = Arc::new(EngineShared {
            store,
            device_name: sanitized_device_name,
            timings,
            socket,
            hot_target: RwLock::new(None),
            session_active: AtomicBool::new(false),
            events: EventBus::new(),
            state: Mutex::new(initial_state),
            worker_wake_lock: Mutex::new(0),
            worker_wake: Condvar::new(),
            shutdown_flag: AtomicBool::new(false),
        });

        let reader_shared = Arc::clone(&shared);
        let reader_handle = thread::Builder::new()
            .name("touchpad-android-udp-reader".to_string())
            .spawn(move || run_udp_reader(reader_shared))?;

        let worker_shared = Arc::clone(&shared);
        let worker_handle = thread::Builder::new()
            .name("touchpad-android-session-worker".to_string())
            .spawn(move || run_session_worker(worker_shared))?;

        info!(
            device = %shared.device_name,
            local_addr = ?shared.socket.local_addr().ok(),
            "AndroidEngine initialized"
        );

        Ok(Self {
            shared,
            threads: Mutex::new(ThreadHandles {
                reader: Some(reader_handle),
                worker: Some(worker_handle),
                discovery: None,
            }),
        })
    }

    /// Returns the local UDP socket address bound by the engine.
    pub fn local_socket_addr(&self) -> Result<SocketAddr, AndroidEngineError> {
        Ok(self.shared.socket.local_addr()?)
    }

    /// Returns a full snapshot of the current UI state.
    pub fn get_state(&self) -> UiStateSnapshot {
        self.shared.snapshot()
    }

    /// Serializes [`Self::get_state`] to JSON.
    pub fn get_state_json(&self) -> String {
        let snapshot = self.get_state();
        serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string())
    }

    /// Waits up to `timeout_ms` for queued UI events using zero-polling `Condvar` wakeup.
    pub fn wait_events(&self, timeout_ms: u64) -> Vec<UiEvent> {
        self.shared.events.wait_events(timeout_ms)
    }

    /// Waits up to `timeout_ms` for queued UI events and returns them as JSON array text.
    pub fn wait_events_json(&self, timeout_ms: u64) -> String {
        self.shared.events.wait_events_json(timeout_ms)
    }

    /// Pushes a toast notification event to the UI event queue.
    pub fn push_toast(&self, message: impl Into<String>) {
        self.shared.events.push(UiEvent::ToastMessage {
            message: message.into(),
        });
    }

    /// Adds a new computer or updates an existing saved computer in persistent storage.
    pub fn add_or_update_computer(
        &self,
        input: ComputerInput,
    ) -> Result<UiStateSnapshot, AndroidEngineError> {
        self.ensure_not_shutdown()?;

        let trimmed_ip = input.ip_address.trim().to_string();
        if trimmed_ip.is_empty() {
            return Err(AndroidEngineError::InvalidInput(
                "computer ip_address must not be empty".to_string(),
            ));
        }

        let port = input.port.filter(|p| *p > 0).unwrap_or(DEFAULT_PORT);
        let requested_id = input
            .id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string);
        let name = {
            let trimmed = input.name.trim();
            if trimmed.is_empty() {
                trimmed_ip.clone()
            } else {
                trimmed.to_string()
            }
        };
        let connection_method_id = input
            .connection_method_id
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| ConnectionMethod::NetworkLan.id().to_string());
        let last_os = input
            .last_os
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let last_privilege = input
            .last_privilege
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let endpoint_key = (trimmed_ip.to_ascii_lowercase(), port);
        let mut target_id = requested_id.clone();

        let saved_cfg = self.shared.store.update(|cfg| {
            let id_idx = requested_id
                .as_ref()
                .and_then(|id| cfg.computers.iter().position(|c| c.id == *id));
            let ep_idx = cfg.computers.iter().position(|c| {
                c.ip_address.trim().eq_ignore_ascii_case(&trimmed_ip)
                    && (if c.port > 0 { c.port } else { DEFAULT_PORT }) == port
            });

            if let Some(idx) = id_idx {
                let existing = &mut cfg.computers[idx];
                existing.name = name.clone();
                existing.ip_address = trimmed_ip.clone();
                existing.port = port;
                existing.connection_method_id = connection_method_id.clone();
                if last_os.is_some() {
                    existing.last_os = last_os.clone();
                }
                if last_privilege.is_some() {
                    existing.last_privilege = last_privilege.clone();
                }
                let kept_id = existing.id.clone();
                target_id = Some(kept_id.clone());

                if let Some(other_idx) = ep_idx {
                    if other_idx != idx {
                        // Keep the explicitly edited record so AndroidConfig::sanitize does not drop it.
                        cfg.computers.remove(other_idx);
                    }
                }
            } else if let Some(idx) = ep_idx {
                let existing = &mut cfg.computers[idx];
                existing.name = name.clone();
                existing.ip_address = trimmed_ip.clone();
                existing.port = port;
                existing.connection_method_id = connection_method_id.clone();
                if last_os.is_some() {
                    existing.last_os = last_os.clone();
                }
                if last_privilege.is_some() {
                    existing.last_privilege = last_privilege.clone();
                }
                target_id = Some(existing.id.clone());
            } else {
                let new_id = requested_id
                    .clone()
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                target_id = Some(new_id.clone());
                cfg.computers.push(SavedComputer {
                    id: new_id,
                    name: name.clone(),
                    ip_address: trimmed_ip.clone(),
                    port,
                    connection_method_id: connection_method_id.clone(),
                    last_os: last_os.clone(),
                    last_privilege: last_privilege.clone(),
                });
            }
        })?;

        let saved_computer = saved_cfg
            .computers
            .iter()
            .find(|c| target_id.as_deref() == Some(c.id.as_str()))
            .or_else(|| {
                saved_cfg.computers.iter().find(|c| {
                    (c.ip_address.to_ascii_lowercase(), c.port) == endpoint_key
                })
            })
            .cloned();

        if let Some(comp) = saved_computer {
            let resolved = resolve_endpoint(&comp.ip_address, comp.port);
            let mut active_hot_update = None;
            {
                let mut state = self.shared.lock_state();
                if let Some(addr) = resolved {
                    state.endpoint_cache.insert(comp.id.clone(), addr);
                } else {
                    state.endpoint_cache.remove(&comp.id);
                }
                if let Some(session) = state.session.as_mut() {
                    if let Some(peer) = session.peer_mut(&comp.id) {
                        peer.name = comp.name.clone();
                        peer.ip_address = comp.ip_address.clone();
                        peer.port = comp.port;
                        peer.addr = resolved;
                        if session.active_computer_id == comp.id {
                            active_hot_update = resolved;
                        }
                    }
                }
            }
            if let Some(addr) = active_hot_update {
                self.shared.set_hot_target(Some(HotTarget {
                    addr,
                    local_sensitivity: saved_cfg.local_sensitivity,
                }));
            }
        }

        self.shared.events.push(UiEvent::StateUpdated);
        Ok(self.get_state())
    }

    /// Parses JSON into [`ComputerInput`] and adds or updates the saved computer.
    pub fn add_or_update_computer_json(
        &self,
        json: &str,
    ) -> Result<UiStateSnapshot, AndroidEngineError> {
        let input: ComputerInput = serde_json::from_str(json)?;
        self.add_or_update_computer(input)
    }

    /// Removes a saved computer and any dangling edge links, updating active sessions safely.
    pub fn remove_computer(&self, computer_id: &str) -> Result<UiStateSnapshot, AndroidEngineError> {
        self.ensure_not_shutdown()?;

        let trimmed_id = computer_id.trim();
        let cfg = self
            .shared
            .store
            .update(|cfg| cfg.computers.retain(|c| c.id != trimmed_id))?;

        let mut packets_to_send: Vec<(SocketAddr, TouchpadPacket)> = Vec::new();
        let mut extra_events: Vec<UiEvent> = Vec::new();
        let mut hot_update: Option<Option<HotTarget>> = None;

        {
            let mut state = self.shared.lock_state();
            state.online_status.remove(trimmed_id);
            state.host_info_map.remove(trimmed_id);
            state.endpoint_cache.remove(trimmed_id);
            if let Some(refresh) = state.pending_refresh.as_mut() {
                refresh.pending_ids.remove(trimmed_id);
            }
            if state.checking_computer_id.as_deref() == Some(trimmed_id) {
                state.checking_computer_id = None;
            }
            if state.pairing_computer_id.as_deref() == Some(trimmed_id) {
                state.pairing_computer_id = None;
            }

            if let Some(mut session) = state.session.take() {
                if let Some(idx) = session.peers.iter().position(|p| p.computer_id == trimmed_id) {
                    let removed_peer = session.peers.remove(idx);
                    if let Some(addr) = removed_peer.addr {
                        packets_to_send.push((
                            addr,
                            TouchpadPacket::ButtonState {
                                button: MouseButton::Left,
                                pressed: false,
                            },
                        ));
                        packets_to_send.push((
                            addr,
                            TouchpadPacket::ButtonState {
                                button: MouseButton::Right,
                                pressed: false,
                            },
                        ));
                        packets_to_send.push((addr, EdgeFlags::default().to_packet(cfg.edge_overshoot_px)));
                    }

                    if session.peers.is_empty()
                        || (session.primary_computer_id == trimmed_id && !session.multi_pc_enabled)
                    {
                        state.checking_computer_id = None;
                        state.pairing_computer_id = None;
                        hot_update = Some(None);
                    } else if session.active_computer_id == trimmed_id {
                        if let Some(fallback) = session
                            .first_other_routable_peer(trimmed_id)
                            .map(|p| (p.computer_id.clone(), p.name.clone(), p.addr))
                        {
                            session.active_computer_id = fallback.0.clone();
                            hot_update = Some(fallback.2.map(|addr| HotTarget {
                                addr,
                                local_sensitivity: cfg.local_sensitivity,
                            }));
                            packets_to_send.extend(session.compute_edge_updates(
                                &cfg.edge_links,
                                cfg.edge_overshoot_px,
                                true,
                            ));
                            extra_events.push(UiEvent::ActiveComputerChanged {
                                computer_id: fallback.0,
                                computer_name: fallback.1,
                                via_edge: None,
                            });
                            state.session = Some(session);
                        } else {
                            state.checking_computer_id = None;
                            state.pairing_computer_id = None;
                            hot_update = Some(None);
                        }
                    } else {
                        packets_to_send.extend(session.compute_edge_updates(
                            &cfg.edge_links,
                            cfg.edge_overshoot_px,
                            true,
                        ));
                        state.session = Some(session);
                    }
                } else {
                    state.session = Some(session);
                }
            }
        }

        if let Some(new_hot) = hot_update {
            self.shared.set_hot_target(new_hot);
        }
        self.shared.send_packets(&packets_to_send);
        for ev in extra_events {
            self.shared.events.push(ev);
        }
        self.shared.events.push(UiEvent::StateUpdated);
        Ok(self.get_state())
    }

    /// Saves Multi-PC spatial edge links and updates `EDGE_CONFIG` for any active session.
    pub fn save_edge_links(&self, links: Vec<EdgeLink>) -> Result<UiStateSnapshot, AndroidEngineError> {
        self.ensure_not_shutdown()?;

        let cfg = self.shared.store.update(|cfg| {
            cfg.edge_links = links;
        })?;

        let packets = {
            let mut state = self.shared.lock_state();
            if let Some(session) = state.session.as_mut() {
                session.compute_edge_updates(&cfg.edge_links, cfg.edge_overshoot_px, true)
            } else {
                Vec::new()
            }
        };

        self.shared.send_packets(&packets);
        self.shared.events.push(UiEvent::StateUpdated);
        Ok(self.get_state())
    }

    /// Parses JSON into `Vec<EdgeLink>` and saves it via [`Self::save_edge_links`].
    pub fn save_edge_links_json(&self, json: &str) -> Result<UiStateSnapshot, AndroidEngineError> {
        let links: Vec<EdgeLink> = serde_json::from_str(json)?;
        self.save_edge_links(links)
    }

    /// Applies partial or full settings JSON to [`AndroidConfig`] and updates live sensitivity/edges.
    pub fn save_settings_json(&self, json: &str) -> Result<UiStateSnapshot, AndroidEngineError> {
        self.ensure_not_shutdown()?;
        let patch: AndroidSettingsPatch = serde_json::from_str(json)?;

        let cfg = self.shared.store.update(|cfg| {
            if let Some(v) = patch.multi_pc_enabled {
                cfg.multi_pc_enabled = v;
            }
            if let Some(v) = patch.hold_duration_ms {
                cfg.hold_duration_ms = v;
            }
            if let Some(v) = patch.haptics_enabled {
                cfg.haptics_enabled = v;
            }
            if let Some(v) = patch.default_landscape {
                cfg.default_landscape = v;
            }
            if let Some(v) = patch.edge_overshoot_px {
                cfg.edge_overshoot_px = v;
            }
            if let Some(v) = patch.local_sensitivity {
                cfg.local_sensitivity = v;
            }
        })?;

        self.shared.update_hot_sensitivity(cfg.local_sensitivity);

        let packets = {
            let mut state = self.shared.lock_state();
            if let Some(session) = state.session.as_mut() {
                session.compute_edge_updates(&cfg.edge_links, cfg.edge_overshoot_px, false)
            } else {
                Vec::new()
            }
        };

        self.shared.send_packets(&packets);
        self.shared.events.push(UiEvent::StateUpdated);
        Ok(self.get_state())
    }

    /// Sends parallel `PING` packets to all saved computers over the shared UDP socket.
    pub fn refresh_status(&self) {
        if self.shared.is_shutdown() {
            return;
        }

        let cfg = self.shared.store.get();
        if cfg.computers.is_empty() {
            let mut state = self.shared.lock_state();
            state.online_status.clear();
            state.pending_refresh = None;
            drop(state);
            self.shared.events.push(UiEvent::StateUpdated);
            return;
        }

        let timestamp = now_millis_i64();
        let mut resolved_targets: Vec<(String, SocketAddr)> = Vec::with_capacity(cfg.computers.len());
        for comp in &cfg.computers {
            let cached = {
                let state = self.shared.lock_state();
                state.endpoint_cache.get(&comp.id).copied()
            };
            let addr = cached.or_else(|| resolve_endpoint(&comp.ip_address, comp.port));
            if let Some(addr) = addr {
                resolved_targets.push((comp.id.clone(), addr));
            }
        }

        let mut pending_ids: HashSet<String> = HashSet::new();
        let mut packets_to_send: Vec<(SocketAddr, TouchpadPacket)> =
            Vec::with_capacity(resolved_targets.len());

        {
            let mut state = self.shared.lock_state();
            let valid_ids: HashSet<String> = cfg.computers.iter().map(|c| c.id.clone()).collect();
            state.online_status.retain(|id, _| valid_ids.contains(id));

            for comp in &cfg.computers {
                if let Some((_, addr)) = resolved_targets.iter().find(|(id, _)| id == &comp.id) {
                    state.endpoint_cache.insert(comp.id.clone(), *addr);
                    pending_ids.insert(comp.id.clone());
                    packets_to_send.push((*addr, TouchpadPacket::Ping { timestamp }));
                } else {
                    state.online_status.insert(comp.id.clone(), false);
                }
            }

            if pending_ids.is_empty() {
                state.pending_refresh = None;
            } else {
                state.pending_refresh = Some(PendingRefresh {
                    pending_ids,
                    timestamp,
                    deadline: Instant::now() + self.shared.timings.refresh_timeout,
                });
            }
        }

        self.shared.send_packets(&packets_to_send);
        if packets_to_send.is_empty() {
            self.shared.events.push(UiEvent::StateUpdated);
        } else {
            self.shared.wake_worker();
        }
    }

    /// Starts an asynchronous LAN broadcast discovery scan (`DISCOVER_REQ`).
    pub fn discover_computers(&self, timeout_ms: u64) {
        if self.shared.is_shutdown() {
            return;
        }

        let prev_handle = {
            let mut threads = self
                .threads
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            threads.discovery.take()
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        {
            let mut state = self.shared.lock_state();
            if let Some(old_cancel) = state.discovery_cancel.take() {
                old_cancel.store(true, Ordering::Release);
            }
            state.discovery_cancel = Some(Arc::clone(&cancel_flag));
            state.is_scanning_network = true;
            state.discovered_computers.clear();
        }
        self.shared.events.push(UiEvent::StateUpdated);

        if let Some(handle) = prev_handle {
            if handle.thread().id() != thread::current().id() {
                let _ = handle.join();
            }
        }

        let shared = Arc::clone(&self.shared);
        let cancel_for_thread = Arc::clone(&cancel_flag);
        let spawned = thread::Builder::new()
            .name("touchpad-android-discovery".to_string())
            .spawn(move || {
                let found = discover_computers_on_lan(
                    DEFAULT_PORT,
                    &shared.device_name,
                    timeout_ms,
                    &cancel_for_thread,
                );

                if cancel_for_thread.load(Ordering::Acquire) || shared.is_shutdown() {
                    return;
                }

                let mut state = shared.lock_state();
                let still_current = state
                    .discovery_cancel
                    .as_ref()
                    .is_some_and(|cur| Arc::ptr_eq(cur, &cancel_for_thread));
                if !still_current {
                    return;
                }
                state.discovered_computers = found;
                state.has_scanned_network = true;
                state.is_scanning_network = false;
                state.discovery_cancel = None;
                drop(state);

                shared.events.push(UiEvent::StateUpdated);
            });

        match spawned {
            Ok(handle) => {
                let mut threads = self
                    .threads
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                threads.discovery = Some(handle);
            }
            Err(err) => {
                warn!(error = %err, "Failed to spawn discovery thread");
                let mut state = self.shared.lock_state();
                state.is_scanning_network = false;
                state.discovery_cancel = None;
                drop(state);
                self.shared.events.push(UiEvent::StateUpdated);
            }
        }
    }

    /// Cancels any active discovery scan and clears discovered results.
    pub fn clear_discovery(&self) {
        if self.shared.is_shutdown() {
            return;
        }
        {
            let mut state = self.shared.lock_state();
            if let Some(cancel) = state.discovery_cancel.take() {
                cancel.store(true, Ordering::Release);
            }
            state.is_scanning_network = false;
            state.discovered_computers.clear();
            state.has_scanned_network = false;
        }
        self.shared.events.push(UiEvent::StateUpdated);
    }

    /// Starts a single-PC or Multi-PC session rooted at `computer_id`.
    ///
    /// In Multi-PC mode, resolves all directly and transitively linked computers and sends
    /// parallel `PAIR_REQ` packets to every computer in the group simultaneously.
    pub fn start_session(&self, computer_id: &str, multi_pc_enabled: bool) {
        if self.shared.is_shutdown() {
            return;
        }

        // Cleanly tear down any previous session first.
        self.teardown_session_internal(false);

        let cfg = self.shared.store.get();
        let group_ids = resolve_session_group(
            computer_id.trim(),
            &cfg.computers,
            &cfg.edge_links,
            multi_pc_enabled,
        );

        if group_ids.is_empty() {
            self.shared.events.push(UiEvent::ToastMessage {
                message: format!("Computer not found: {computer_id}"),
            });
            self.shared.events.push(UiEvent::StateUpdated);
            return;
        }

        let mut peers: Vec<SessionPeer> = Vec::with_capacity(group_ids.len());
        let mut resolved_cache_updates: Vec<(String, SocketAddr)> = Vec::new();
        for id in &group_ids {
            let Some(comp) = cfg.computers.iter().find(|c| c.id == *id) else {
                continue;
            };
            let cached = {
                let state = self.shared.lock_state();
                state.endpoint_cache.get(&comp.id).copied()
            };
            let addr = cached.or_else(|| resolve_endpoint(&comp.ip_address, comp.port));
            if let Some(addr) = addr {
                resolved_cache_updates.push((comp.id.clone(), addr));
            }
            peers.push(SessionPeer {
                computer_id: comp.id.clone(),
                name: comp.name.clone(),
                ip_address: comp.ip_address.clone(),
                port: comp.port,
                addr,
                online: false,
                pairing_status: None,
                last_seen_at: None,
                awaiting_heartbeat_reply: false,
                missed_heartbeats: 0,
                last_sent_edge_config: None,
            });
        }

        let Some(primary_peer) = peers.first() else {
            return;
        };

        if primary_peer.addr.is_none() {
            let offline_id = primary_peer.computer_id.clone();
            let offline_name = primary_peer.name.clone();
            {
                let mut state = self.shared.lock_state();
                state.online_status.insert(offline_id.clone(), false);
            }
            self.shared.events.push(UiEvent::ComputerOffline {
                computer_id: offline_id,
                computer_name: offline_name,
            });
            self.shared.events.push(UiEvent::StateUpdated);
            return;
        }

        let now = Instant::now();
        let primary_id = primary_peer.computer_id.clone();
        let pair_packet = TouchpadPacket::PairRequest {
            client_id: cfg.client_id.clone(),
            client_name: self.shared.device_name.clone(),
        };
        let mut initial_packets: Vec<(SocketAddr, TouchpadPacket)> = Vec::with_capacity(peers.len());
        for peer in &peers {
            if let Some(addr) = peer.addr {
                initial_packets.push((addr, pair_packet.clone()));
            }
        }

        let session = MultiPcSession {
            phase: SessionPhase::Connecting,
            primary_computer_id: primary_id.clone(),
            active_computer_id: primary_id.clone(),
            multi_pc_enabled,
            edge_locked: false,
            peers,
            started_at: now,
            next_pair_poll_at: now + self.shared.timings.pair_poll_interval,
            missed_pair_polls: 0,
            next_heartbeat_at: now + self.shared.timings.heartbeat_interval,
            last_edge_transition_at: None,
        };

        {
            let mut state = self.shared.lock_state();
            for (id, addr) in resolved_cache_updates {
                state.endpoint_cache.insert(id, addr);
            }
            state.checking_computer_id = Some(primary_id);
            state.pairing_computer_id = None;
            state.session = Some(session);
        }

        self.shared.send_packets(&initial_packets);
        self.shared.events.push(UiEvent::StateUpdated);
        self.shared.wake_worker();
    }

    /// Cancels an ongoing connection or pairing approval prompt.
    pub fn cancel_pairing(&self) {
        if self.shared.is_shutdown() {
            return;
        }
        let changed = {
            let mut state = self.shared.lock_state();
            let was_pending = state.session.as_ref().is_some_and(|s| {
                matches!(
                    s.phase,
                    SessionPhase::Connecting | SessionPhase::PairingPending
                )
            });
            if was_pending {
                state.session = None;
            }
            let had_flags =
                state.checking_computer_id.take().is_some() || state.pairing_computer_id.take().is_some();
            was_pending || had_flags
        };
        if changed {
            self.shared.set_hot_target(None);
            self.shared.events.push(UiEvent::StateUpdated);
        }
    }

    /// Releases held mouse buttons, disables edge triggers on all session PCs, and exits the session.
    pub fn exit_session(&self) {
        if self.shared.is_shutdown() {
            return;
        }
        self.teardown_session_internal(true);
    }

    /// Manually switches active cursor control to `target_computer_id` within the active session.
    pub fn switch_active_computer(&self, target_computer_id: &str) -> bool {
        if self.shared.is_shutdown() {
            return false;
        }

        let target_id = target_computer_id.trim();
        let cfg = self.shared.store.get();

        let (release_addr, new_id, new_name, new_addr) = {
            let mut state = self.shared.lock_state();
            let Some(session) = state.session.as_mut() else {
                return false;
            };
            if session.phase != SessionPhase::Active {
                return false;
            }
            if session.active_computer_id == target_id {
                return session.active_peer().is_some_and(SessionPeer::is_routable);
            }
            let Some((new_name, new_addr)) = session
                .peer(target_id)
                .filter(|p| p.is_routable())
                .and_then(|p| p.addr.map(|addr| (p.name.clone(), addr)))
            else {
                return false;
            };
            let release_addr = session.active_peer().and_then(|p| p.addr);
            session.active_computer_id = target_id.to_string();
            (release_addr, target_id.to_string(), new_name, new_addr)
        };

        self.shared.set_hot_target(Some(HotTarget {
            addr: new_addr,
            local_sensitivity: cfg.local_sensitivity,
        }));

        if let Some(old_addr) = release_addr {
            self.shared.send_release_buttons(old_addr);
        }

        self.shared.events.push(UiEvent::ActiveComputerChanged {
            computer_id: new_id,
            computer_name: new_name,
            via_edge: None,
        });
        self.shared.events.push(UiEvent::StateUpdated);
        true
    }

    /// Locks or unlocks Multi-PC screen edge transitions and pushes updated `EDGE_CONFIG` packets.
    pub fn set_edge_lock(&self, locked: bool) {
        if self.shared.is_shutdown() {
            return;
        }

        let cfg = self.shared.store.get();
        let (changed, packets) = {
            let mut state = self.shared.lock_state();
            let Some(session) = state.session.as_mut() else {
                return;
            };
            let changed = session.edge_locked != locked;
            session.edge_locked = locked;
            let packets = session.compute_edge_updates(&cfg.edge_links, cfg.edge_overshoot_px, true);
            (changed || !packets.is_empty(), packets)
        };

        self.shared.send_packets(&packets);
        if changed {
            self.shared.events.push(UiEvent::StateUpdated);
        }
    }

    /// Hot-path cursor move dispatch to the currently active computer.
    #[inline]
    pub fn send_move(&self, dx: f32, dy: f32) {
        if !self.shared.session_active.load(Ordering::Acquire)
            || !dx.is_finite()
            || !dy.is_finite()
        {
            return;
        }
        let target = *self
            .shared
            .hot_target
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(HotTarget {
            addr,
            local_sensitivity,
        }) = target
        else {
            return;
        };
        let packet = TouchpadPacket::Move {
            dx: dx * local_sensitivity,
            dy: dy * local_sensitivity,
        };
        let _ = self.shared.socket.send_to(&packet.to_bytes(), addr);
    }

    /// Hot-path mouse click dispatch to the currently active computer.
    #[inline]
    pub fn send_click(&self, button: MouseButton) {
        if !self.shared.session_active.load(Ordering::Acquire) {
            return;
        }
        let target = *self
            .shared
            .hot_target
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(HotTarget { addr, .. }) = target else {
            return;
        };
        let packet = TouchpadPacket::Click { button };
        let _ = self.shared.socket.send_to(&packet.to_bytes(), addr);
    }

    /// Hot-path button press/release dispatch to the currently active computer.
    #[inline]
    pub fn send_button_state(&self, button: MouseButton, pressed: bool) {
        if !self.shared.session_active.load(Ordering::Acquire) {
            return;
        }
        let target = *self
            .shared
            .hot_target
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(HotTarget { addr, .. }) = target else {
            return;
        };
        let packet = TouchpadPacket::ButtonState { button, pressed };
        let _ = self.shared.socket.send_to(&packet.to_bytes(), addr);
    }

    /// Hot-path two-finger scroll dispatch to the currently active computer.
    #[inline]
    pub fn send_scroll(&self, delta_y: f32) {
        if !self.shared.session_active.load(Ordering::Acquire) || !delta_y.is_finite() {
            return;
        }
        let target = *self
            .shared
            .hot_target
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(HotTarget { addr, .. }) = target else {
            return;
        };
        let packet = TouchpadPacket::Scroll { delta_y };
        let _ = self.shared.socket.send_to(&packet.to_bytes(), addr);
    }

    /// Cleanly tears down active sessions, discovery, reader, and worker threads.
    pub fn shutdown(&self) {
        if self.shared.shutdown_flag.swap(true, Ordering::AcqRel) {
            return;
        }

        self.teardown_session_internal(false);

        {
            let mut state = self.shared.lock_state();
            if let Some(cancel) = state.discovery_cancel.take() {
                cancel.store(true, Ordering::Release);
            }
            state.is_scanning_network = false;
        }

        self.shared.events.shutdown();
        self.shared.wake_worker();

        let (reader, worker, discovery) = {
            let mut threads = self
                .threads
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                threads.reader.take(),
                threads.worker.take(),
                threads.discovery.take(),
            )
        };

        let current_id = thread::current().id();
        for handle in [reader, worker, discovery].into_iter().flatten() {
            if handle.thread().id() != current_id {
                let _ = handle.join();
            }
        }
        info!("AndroidEngine shut down");
    }

    fn ensure_not_shutdown(&self) -> Result<(), AndroidEngineError> {
        if self.shared.is_shutdown() {
            Err(AndroidEngineError::Shutdown)
        } else {
            Ok(())
        }
    }

    fn teardown_session_internal(&self, emit_state_update: bool) {
        self.shared.set_hot_target(None);
        let cfg = self.shared.store.get();
        let mut packets_to_send: Vec<(SocketAddr, TouchpadPacket)> = Vec::new();
        let mut had_session = false;

        {
            let mut state = self.shared.lock_state();
            if let Some(session) = state.session.take() {
                had_session = true;
                if let Some(active_addr) = session.active_peer().and_then(|p| p.addr) {
                    packets_to_send.push((
                        active_addr,
                        TouchpadPacket::ButtonState {
                            button: MouseButton::Left,
                            pressed: false,
                        },
                    ));
                    packets_to_send.push((
                        active_addr,
                        TouchpadPacket::ButtonState {
                            button: MouseButton::Right,
                            pressed: false,
                        },
                    ));
                }
                let clear_edges = EdgeFlags::default().to_packet(cfg.edge_overshoot_px);
                for peer in &session.peers {
                    if let Some(addr) = peer.addr {
                        packets_to_send.push((addr, clear_edges.clone()));
                    }
                }
            }
            if state.checking_computer_id.take().is_some() {
                had_session = true;
            }
            if state.pairing_computer_id.take().is_some() {
                had_session = true;
            }
        }

        self.shared.send_packets(&packets_to_send);
        if emit_state_update && had_session {
            self.shared.events.push(UiEvent::StateUpdated);
        }
    }
}

impl Drop for AndroidEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl EngineShared {
    #[inline]
    fn is_shutdown(&self) -> bool {
        self.shutdown_flag.load(Ordering::Acquire)
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, RuntimeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_hot_target(&self, target: Option<HotTarget>) {
        self.session_active
            .store(target.is_some(), Ordering::Release);
        let mut guard = self
            .hot_target
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = target;
    }

    fn update_hot_sensitivity(&self, sensitivity: f32) {
        let mut guard = self
            .hot_target
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(target) = guard.as_mut() {
            target.local_sensitivity = sensitivity;
        }
    }

    fn wake_worker(&self) {
        let mut guard = self
            .worker_wake_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = guard.wrapping_add(1);
        self.worker_wake.notify_all();
    }

    fn send_packet(&self, addr: SocketAddr, packet: &TouchpadPacket) {
        let _ = self.socket.send_to(&packet.to_bytes(), addr);
    }

    fn send_packets(&self, packets: &[(SocketAddr, TouchpadPacket)]) {
        for (addr, packet) in packets {
            self.send_packet(*addr, packet);
        }
    }

    fn send_release_buttons(&self, addr: SocketAddr) {
        self.send_packet(
            addr,
            &TouchpadPacket::ButtonState {
                button: MouseButton::Left,
                pressed: false,
            },
        );
        self.send_packet(
            addr,
            &TouchpadPacket::ButtonState {
                button: MouseButton::Right,
                pressed: false,
            },
        );
    }

    fn snapshot(&self) -> UiStateSnapshot {
        let cfg = self.store.get();
        let state = self.lock_state();
        UiStateSnapshot {
            client_id: cfg.client_id.clone(),
            device_name: self.device_name.clone(),
            computers: cfg.computers.clone(),
            online_status: state.online_status.clone(),
            host_info_map: state.host_info_map.clone(),
            discovered_computers: state.discovered_computers.clone(),
            is_scanning_network: state.is_scanning_network,
            has_scanned_network: state.has_scanned_network,
            edge_links: cfg.edge_links.clone(),
            settings: AndroidSettingsDto::from(&cfg),
            active_session: state
                .session
                .as_ref()
                .map(|s| s.to_snapshot(&cfg.edge_links)),
            checking_computer_id: state.checking_computer_id.clone(),
            pairing_computer_id: state.pairing_computer_id.clone(),
        }
    }
}

fn run_udp_reader(shared: Arc<EngineShared>) {
    let mut buf = [0u8; RECEIVE_BUFFER_BYTES];
    while !shared.is_shutdown() {
        match shared.socket.recv_from(&mut buf) {
            Ok((len, src_addr)) => {
                if let Some(packet) = TouchpadPacket::from_bytes(&buf[..len]) {
                    handle_incoming_packet(&shared, src_addr, packet);
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::WouldBlock
                        | ErrorKind::TimedOut
                        | ErrorKind::Interrupted
                        | ErrorKind::ConnectionReset
                ) || err.raw_os_error() == Some(10054) => {}
            Err(err) => {
                if !shared.is_shutdown() {
                    debug!(error = %err, "UDP reader socket error");
                }
            }
        }
    }
}

fn handle_incoming_packet(shared: &Arc<EngineShared>, src_addr: SocketAddr, packet: TouchpadPacket) {
    let now = Instant::now();
    let cfg = shared.store.get();

    match packet {
        TouchpadPacket::Pong {
            timestamp,
            server_name: _,
        } => {
            let mut events: Vec<UiEvent> = Vec::new();
            let mut state_changed = false;
            {
                let mut state = shared.lock_state();
                let Some(comp_id) = find_computer_id_by_addr(&state, &cfg, src_addr) else {
                    return;
                };
                let prev_online = state.online_status.insert(comp_id.clone(), true);
                if prev_online != Some(true) {
                    state_changed = true;
                }

                if let Some(refresh) = state.pending_refresh.as_mut() {
                    if refresh.timestamp == timestamp && refresh.pending_ids.remove(&comp_id) {
                        state_changed = true;
                    }
                    if refresh.pending_ids.is_empty() {
                        state.pending_refresh = None;
                    }
                }

                if let Some(session) = state.session.as_mut() {
                    if let Some(peer) = session.peer_mut(&comp_id) {
                        peer.online = true;
                        peer.last_seen_at = Some(now);
                        peer.awaiting_heartbeat_reply = false;
                        peer.missed_heartbeats = 0;
                    }
                }
            }
            if state_changed {
                events.push(UiEvent::StateUpdated);
            }
            for ev in events {
                shared.events.push(ev);
            }
        }

        TouchpadPacket::HostInfo {
            os,
            width,
            height,
            privilege,
        } => {
            let comp_id = {
                let state = shared.lock_state();
                find_computer_id_by_addr(&state, &cfg, src_addr)
            };
            let Some(comp_id) = comp_id else {
                return;
            };

            let info = HostInfoSnapshot {
                os,
                width,
                height,
                privilege,
            };

            let changed_map = {
                let mut state = shared.lock_state();
                let prev = state.host_info_map.insert(comp_id.clone(), info);
                prev != Some(HostInfoSnapshot {
                    os,
                    width,
                    height,
                    privilege,
                })
            };

            let os_str = host_os_to_str(os).to_string();
            let priv_str = privilege_mode_to_str(privilege).to_string();
            let needs_persist = cfg.computers.iter().any(|c| {
                c.id == comp_id
                    && (c.last_os.as_deref() != Some(os_str.as_str())
                        || c.last_privilege.as_deref() != Some(priv_str.as_str()))
            });

            if needs_persist {
                let _ = shared.store.update(|c| {
                    if let Some(entry) = c.computers.iter_mut().find(|entry| entry.id == comp_id) {
                        entry.last_os = Some(os_str.clone());
                        entry.last_privilege = Some(priv_str.clone());
                    }
                });
            }

            if changed_map || needs_persist {
                shared.events.push(UiEvent::StateUpdated);
            }
        }

        TouchpadPacket::PairResponse { status, .. } => {
            handle_pair_response(shared, &cfg, src_addr, status, now);
        }

        TouchpadPacket::EdgeHit { edge, ratio } => {
            handle_edge_hit(shared, &cfg, src_addr, edge, ratio, now);
        }

        _ => {}
    }
}

fn handle_pair_response(
    shared: &Arc<EngineShared>,
    cfg: &AndroidConfig,
    src_addr: SocketAddr,
    status: PairingStatus,
    now: Instant,
) {
    let mut packets_to_send: Vec<(SocketAddr, TouchpadPacket)> = Vec::new();
    let mut events_to_push: Vec<UiEvent> = Vec::new();
    let mut hot_update: Option<Option<HotTarget>> = None;
    let mut wake_worker = false;

    {
        let mut state = shared.lock_state();
        let Some(sender_id) = find_computer_id_by_addr(&state, cfg, src_addr) else {
            return;
        };
        state.online_status.insert(sender_id.clone(), true);

        let Some(mut session) = state.session.take() else {
            return;
        };
        if session.peer(&sender_id).is_none() {
            state.session = Some(session);
            return;
        }

        let is_primary = sender_id == session.primary_computer_id;

        match session.phase {
            SessionPhase::Connecting | SessionPhase::PairingPending => {
                if is_primary {
                    match status {
                        PairingStatus::Approved => {
                            let (primary_name, primary_addr) = {
                                let peer = session.peer_mut(&sender_id).unwrap();
                                peer.online = true;
                                peer.pairing_status = Some(PairingStatus::Approved);
                                peer.last_seen_at = Some(now);
                                peer.awaiting_heartbeat_reply = false;
                                peer.missed_heartbeats = 0;
                                (peer.name.clone(), peer.addr)
                            };
                            session.phase = SessionPhase::Active;
                            session.active_computer_id = sender_id.clone();
                            session.next_heartbeat_at = now + shared.timings.heartbeat_interval;
                            state.checking_computer_id = None;
                            state.pairing_computer_id = None;
                            wake_worker = true;

                            if let Some(addr) = primary_addr {
                                hot_update = Some(Some(HotTarget {
                                    addr,
                                    local_sensitivity: cfg.local_sensitivity,
                                }));
                            }
                            packets_to_send.extend(session.compute_edge_updates(
                                &cfg.edge_links,
                                cfg.edge_overshoot_px,
                                true,
                            ));
                            events_to_push.push(UiEvent::ActiveComputerChanged {
                                computer_id: sender_id,
                                computer_name: primary_name,
                                via_edge: None,
                            });
                            events_to_push.push(UiEvent::StateUpdated);
                            state.session = Some(session);
                        }
                        PairingStatus::Pending => {
                            let was_connecting = session.phase == SessionPhase::Connecting;
                            let primary_name = {
                                let peer = session.peer_mut(&sender_id).unwrap();
                                peer.online = true;
                                peer.pairing_status = Some(PairingStatus::Pending);
                                peer.last_seen_at = Some(now);
                                peer.name.clone()
                            };
                            session.phase = SessionPhase::PairingPending;
                            session.missed_pair_polls = 0;
                            state.checking_computer_id = None;
                            state.pairing_computer_id = Some(sender_id.clone());
                            if was_connecting {
                                session.next_pair_poll_at =
                                    now + shared.timings.pair_poll_interval;
                                wake_worker = true;
                                events_to_push.push(UiEvent::PairingPending {
                                    computer_id: sender_id,
                                    computer_name: primary_name,
                                });
                            }
                            events_to_push.push(UiEvent::StateUpdated);
                            state.session = Some(session);
                        }
                        PairingStatus::Rejected => {
                            let primary_name = session
                                .peer(&sender_id)
                                .map(|p| p.name.clone())
                                .unwrap_or_default();
                            state.checking_computer_id = None;
                            state.pairing_computer_id = None;
                            hot_update = Some(None);
                            events_to_push.push(UiEvent::PairingRejected {
                                computer_id: sender_id,
                                computer_name: primary_name,
                            });
                            events_to_push.push(UiEvent::StateUpdated);
                        }
                    }
                } else {
                    // Secondary peer responded while waiting for primary.
                    let peer = session.peer_mut(&sender_id).unwrap();
                    peer.last_seen_at = Some(now);
                    peer.awaiting_heartbeat_reply = false;
                    peer.missed_heartbeats = 0;
                    peer.pairing_status = Some(status);
                    peer.online = status != PairingStatus::Rejected;
                    if status == PairingStatus::Rejected {
                        events_to_push.push(UiEvent::PairingRejected {
                            computer_id: sender_id,
                            computer_name: peer.name.clone(),
                        });
                    }
                    events_to_push.push(UiEvent::StateUpdated);
                    state.session = Some(session);
                }
            }

            SessionPhase::Active => {
                let (was_routable, peer_name, peer_addr) = {
                    let peer = session.peer_mut(&sender_id).unwrap();
                    let was = peer.is_routable();
                    peer.last_seen_at = Some(now);
                    peer.awaiting_heartbeat_reply = false;
                    peer.missed_heartbeats = 0;
                    peer.pairing_status = Some(status);
                    match status {
                        PairingStatus::Approved => {
                            peer.online = true;
                        }
                        PairingStatus::Pending | PairingStatus::Rejected => {
                            peer.online = false;
                        }
                    }
                    (was, peer.name.clone(), peer.addr)
                };

                match status {
                    PairingStatus::Approved => {
                        if !was_routable {
                            packets_to_send.extend(session.compute_edge_updates(
                                &cfg.edge_links,
                                cfg.edge_overshoot_px,
                                false,
                            ));
                            events_to_push.push(UiEvent::StateUpdated);
                        }
                        state.session = Some(session);
                    }
                    PairingStatus::Pending | PairingStatus::Rejected => {
                        if let Some(addr) = peer_addr {
                            packets_to_send.push((
                                addr,
                                TouchpadPacket::ButtonState {
                                    button: MouseButton::Left,
                                    pressed: false,
                                },
                            ));
                            packets_to_send.push((
                                addr,
                                TouchpadPacket::ButtonState {
                                    button: MouseButton::Right,
                                    pressed: false,
                                },
                            ));
                            packets_to_send
                                .push((addr, EdgeFlags::default().to_packet(cfg.edge_overshoot_px)));
                        }

                        if session.active_computer_id == sender_id {
                            if session.multi_pc_enabled {
                                if let Some((fallback_id, fallback_name, fallback_addr)) = session
                                    .first_other_routable_peer(&sender_id)
                                    .and_then(|p| {
                                        p.addr.map(|a| (p.computer_id.clone(), p.name.clone(), a))
                                    })
                                {
                                    session.active_computer_id = fallback_id.clone();
                                    hot_update = Some(Some(HotTarget {
                                        addr: fallback_addr,
                                        local_sensitivity: cfg.local_sensitivity,
                                    }));
                                    packets_to_send.extend(session.compute_edge_updates(
                                        &cfg.edge_links,
                                        cfg.edge_overshoot_px,
                                        true,
                                    ));
                                    if status == PairingStatus::Rejected {
                                        events_to_push.push(UiEvent::PairingRejected {
                                            computer_id: sender_id,
                                            computer_name: peer_name.clone(),
                                        });
                                    }
                                    events_to_push.push(UiEvent::ActiveComputerChanged {
                                        computer_id: fallback_id,
                                        computer_name: fallback_name.clone(),
                                        via_edge: None,
                                    });
                                    events_to_push.push(UiEvent::ToastMessage {
                                        message: format!(
                                            "\"{peer_name}\" disconnected; switched to \"{fallback_name}\""
                                        ),
                                    });
                                    events_to_push.push(UiEvent::StateUpdated);
                                    state.session = Some(session);
                                } else {
                                    hot_update = Some(None);
                                    state.checking_computer_id = None;
                                    state.pairing_computer_id = None;
                                    if status == PairingStatus::Rejected {
                                        events_to_push.push(UiEvent::PairingRejected {
                                            computer_id: sender_id,
                                            computer_name: peer_name.clone(),
                                        });
                                    }
                                    events_to_push.push(UiEvent::SessionDisconnected {
                                        reason: format!("Pairing lost on \"{peer_name}\""),
                                    });
                                    events_to_push.push(UiEvent::StateUpdated);
                                }
                            } else {
                                hot_update = Some(None);
                                state.checking_computer_id = None;
                                state.pairing_computer_id = None;
                                if status == PairingStatus::Rejected {
                                    events_to_push.push(UiEvent::PairingRejected {
                                        computer_id: sender_id,
                                        computer_name: peer_name.clone(),
                                    });
                                }
                                events_to_push.push(UiEvent::SessionDisconnected {
                                    reason: format!("Pairing lost on \"{peer_name}\""),
                                });
                                events_to_push.push(UiEvent::StateUpdated);
                            }
                        } else if was_routable {
                            packets_to_send.extend(session.compute_edge_updates(
                                &cfg.edge_links,
                                cfg.edge_overshoot_px,
                                false,
                            ));
                            if status == PairingStatus::Rejected {
                                events_to_push.push(UiEvent::PairingRejected {
                                    computer_id: sender_id,
                                    computer_name: peer_name,
                                });
                            }
                            events_to_push.push(UiEvent::StateUpdated);
                            state.session = Some(session);
                        } else {
                            state.session = Some(session);
                        }
                    }
                }
            }
        }
    }

    if let Some(new_hot) = hot_update {
        shared.set_hot_target(new_hot);
    }
    shared.send_packets(&packets_to_send);
    if wake_worker {
        shared.wake_worker();
    }
    for ev in events_to_push {
        shared.events.push(ev);
    }
}

fn handle_edge_hit(
    shared: &Arc<EngineShared>,
    cfg: &AndroidConfig,
    src_addr: SocketAddr,
    edge: touchpad_protocol::ScreenEdge,
    ratio: f32,
    now: Instant,
) {
    let clamped_ratio = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.5
    };

    let transition: Option<(
        Option<SocketAddr>,
        SocketAddr,
        String,
        String,
        touchpad_protocol::ScreenEdge,
    )> = {
        let mut state = shared.lock_state();
        let Some(sender_id) = find_computer_id_by_addr(&state, cfg, src_addr) else {
            return;
        };
        let Some(session) = state.session.as_mut() else {
            return;
        };

        if session.phase != SessionPhase::Active
            || !session.multi_pc_enabled
            || session.edge_locked
            || sender_id != session.active_computer_id
        {
            return;
        }

        if let Some(prev_at) = session.last_edge_transition_at {
            if now.saturating_duration_since(prev_at) < shared.timings.edge_cooldown {
                return;
            }
        }

        let Some((target_id, entry_edge)) = find_edge_target(&sender_id, edge, &cfg.edge_links)
        else {
            return;
        };

        let Some((new_addr, new_name)) = session
            .peer(&target_id)
            .filter(|p| p.is_routable())
            .and_then(|p| p.addr.map(|addr| (addr, p.name.clone())))
        else {
            return;
        };

        let old_addr = session.active_peer().and_then(|p| p.addr);
        session.active_computer_id = target_id.clone();
        session.last_edge_transition_at = Some(now);
        Some((old_addr, new_addr, target_id, new_name, entry_edge))
    };

    let Some((old_addr, new_addr, target_id, new_name, entry_edge)) = transition else {
        return;
    };

    shared.set_hot_target(Some(HotTarget {
        addr: new_addr,
        local_sensitivity: cfg.local_sensitivity,
    }));

    if let Some(old_addr) = old_addr {
        shared.send_release_buttons(old_addr);
        shared.send_packet(old_addr, &TouchpadPacket::CursorLeave { edge });
    }
    shared.send_packet(
        new_addr,
        &TouchpadPacket::CursorEnter {
            edge: entry_edge,
            ratio: clamped_ratio,
        },
    );

    shared.events.push(UiEvent::ActiveComputerChanged {
        computer_id: target_id,
        computer_name: new_name,
        via_edge: Some(edge),
    });
    shared.events.push(UiEvent::StateUpdated);
}

fn run_session_worker(shared: Arc<EngineShared>) {
    while !shared.is_shutdown() {
        let now = Instant::now();
        let cfg = shared.store.get();
        let mut packets_to_send: Vec<(SocketAddr, TouchpadPacket)> = Vec::new();
        let mut events_to_push: Vec<UiEvent> = Vec::new();
        let mut hot_update: Option<Option<HotTarget>> = None;
        let mut next_wakeup = now + shared.timings.worker_tick_interval;

        {
            let mut state = shared.lock_state();

            // 1. Complete timed-out status refresh pings.
            if let Some(refresh) = state.pending_refresh.as_ref() {
                if now >= refresh.deadline {
                    let timed_out_ids: Vec<String> = refresh.pending_ids.iter().cloned().collect();
                    state.pending_refresh = None;
                    for id in timed_out_ids {
                        if cfg.computers.iter().any(|c| c.id == id) {
                            state.online_status.insert(id, false);
                        }
                    }
                    events_to_push.push(UiEvent::StateUpdated);
                } else {
                    next_wakeup = next_wakeup.min(refresh.deadline);
                }
            }

            // 2. Drive session connection, pairing polling, and Multi-PC heartbeats.
            if let Some(mut session) = state.session.take() {
                let mut keep_session = true;
                match session.phase {
                    SessionPhase::Connecting => {
                        let deadline = session.started_at + shared.timings.initial_pair_timeout;
                        if now >= deadline {
                            let primary_id = session.primary_computer_id.clone();
                            let primary_name = session
                                .peer(&primary_id)
                                .map(|p| p.name.clone())
                                .unwrap_or_default();
                            state.online_status.insert(primary_id.clone(), false);
                            state.checking_computer_id = None;
                            state.pairing_computer_id = None;
                            hot_update = Some(None);
                            keep_session = false;
                            events_to_push.push(UiEvent::ComputerOffline {
                                computer_id: primary_id,
                                computer_name: primary_name,
                            });
                            events_to_push.push(UiEvent::StateUpdated);
                        } else {
                            next_wakeup = next_wakeup.min(deadline);
                        }
                    }

                    SessionPhase::PairingPending => {
                        if now >= session.next_pair_poll_at {
                            let prev_poll_at =
                                session.next_pair_poll_at - shared.timings.pair_poll_interval;
                            let primary_replied = session
                                .peer(&session.primary_computer_id)
                                .and_then(|p| p.last_seen_at)
                                .is_some_and(|seen| seen >= prev_poll_at);

                            if primary_replied {
                                session.missed_pair_polls = 0;
                            } else {
                                session.missed_pair_polls = session.missed_pair_polls.saturating_add(1);
                            }

                            if session.missed_pair_polls >= 3 {
                                let primary_id = session.primary_computer_id.clone();
                                let primary_name = session
                                    .peer(&primary_id)
                                    .map(|p| p.name.clone())
                                    .unwrap_or_default();
                                state.online_status.insert(primary_id.clone(), false);
                                state.checking_computer_id = None;
                                state.pairing_computer_id = None;
                                hot_update = Some(None);
                                keep_session = false;
                                events_to_push.push(UiEvent::ComputerOffline {
                                    computer_id: primary_id,
                                    computer_name: primary_name,
                                });
                                events_to_push.push(UiEvent::StateUpdated);
                            } else {
                                session.next_pair_poll_at = now + shared.timings.pair_poll_interval;
                                next_wakeup = next_wakeup.min(session.next_pair_poll_at);
                                let pair_req = TouchpadPacket::PairRequest {
                                    client_id: cfg.client_id.clone(),
                                    client_name: shared.device_name.clone(),
                                };
                                for peer in &session.peers {
                                    if peer.computer_id == session.primary_computer_id
                                        || (peer.pairing_status != Some(PairingStatus::Approved)
                                            && peer.pairing_status != Some(PairingStatus::Rejected))
                                    {
                                        if let Some(addr) = peer.addr {
                                            packets_to_send.push((addr, pair_req.clone()));
                                        }
                                    }
                                }
                            }
                        } else {
                            next_wakeup = next_wakeup.min(session.next_pair_poll_at);
                        }
                    }

                    SessionPhase::Active => {
                        if now >= session.next_heartbeat_at {
                            session.next_heartbeat_at = now + shared.timings.heartbeat_interval;
                            next_wakeup = next_wakeup.min(session.next_heartbeat_at);

                            let mut newly_offline: Vec<(String, String, Option<SocketAddr>)> =
                                Vec::new();
                            for peer in &mut session.peers {
                                if peer.awaiting_heartbeat_reply {
                                    peer.missed_heartbeats =
                                        peer.missed_heartbeats.saturating_add(1);
                                    if peer.missed_heartbeats >= 2 {
                                        let was_online = peer.online;
                                        peer.online = false;
                                        peer.awaiting_heartbeat_reply = false;
                                        if was_online {
                                            newly_offline.push((
                                                peer.computer_id.clone(),
                                                peer.name.clone(),
                                                peer.addr,
                                            ));
                                        }
                                    }
                                }
                            }

                            if !newly_offline.is_empty() {
                                for (id, _, addr) in &newly_offline {
                                    state.online_status.insert(id.clone(), false);
                                    if let Some(addr) = addr {
                                        packets_to_send.push((
                                            *addr,
                                            EdgeFlags::default().to_packet(cfg.edge_overshoot_px),
                                        ));
                                    }
                                }

                                let active_id = session.active_computer_id.clone();
                                if let Some((_, active_name, active_addr)) = newly_offline
                                    .iter()
                                    .find(|(id, _, _)| *id == active_id)
                                    .cloned()
                                {
                                    if let Some(addr) = active_addr {
                                        packets_to_send.push((
                                            addr,
                                            TouchpadPacket::ButtonState {
                                                button: MouseButton::Left,
                                                pressed: false,
                                            },
                                        ));
                                        packets_to_send.push((
                                            addr,
                                            TouchpadPacket::ButtonState {
                                                button: MouseButton::Right,
                                                pressed: false,
                                            },
                                        ));
                                    }

                                    if session.multi_pc_enabled {
                                        if let Some((fallback_id, fallback_name, fallback_addr)) =
                                            session
                                                .first_other_routable_peer(&active_id)
                                                .and_then(|p| {
                                                    p.addr.map(|a| {
                                                        (
                                                            p.computer_id.clone(),
                                                            p.name.clone(),
                                                            a,
                                                        )
                                                    })
                                                })
                                        {
                                            session.active_computer_id = fallback_id.clone();
                                            hot_update = Some(Some(HotTarget {
                                                addr: fallback_addr,
                                                local_sensitivity: cfg.local_sensitivity,
                                            }));
                                            packets_to_send.extend(session.compute_edge_updates(
                                                &cfg.edge_links,
                                                cfg.edge_overshoot_px,
                                                true,
                                            ));
                                            events_to_push.push(UiEvent::ComputerOffline {
                                                computer_id: active_id,
                                                computer_name: active_name.clone(),
                                            });
                                            events_to_push.push(UiEvent::ActiveComputerChanged {
                                                computer_id: fallback_id,
                                                computer_name: fallback_name.clone(),
                                                via_edge: None,
                                            });
                                            events_to_push.push(UiEvent::ToastMessage {
                                                message: format!(
                                                    "\"{active_name}\" offline; switched to \"{fallback_name}\""
                                                ),
                                            });
                                            events_to_push.push(UiEvent::StateUpdated);
                                        } else {
                                            hot_update = Some(None);
                                            state.checking_computer_id = None;
                                            state.pairing_computer_id = None;
                                            keep_session = false;
                                            events_to_push.push(UiEvent::SessionDisconnected {
                                                reason: format!(
                                                    "Connection lost to \"{active_name}\""
                                                ),
                                            });
                                            events_to_push.push(UiEvent::StateUpdated);
                                        }
                                    } else {
                                        hot_update = Some(None);
                                        state.checking_computer_id = None;
                                        state.pairing_computer_id = None;
                                        keep_session = false;
                                        events_to_push.push(UiEvent::SessionDisconnected {
                                            reason: format!(
                                                "Connection lost to \"{active_name}\""
                                            ),
                                        });
                                        events_to_push.push(UiEvent::StateUpdated);
                                    }
                                } else {
                                    packets_to_send.extend(session.compute_edge_updates(
                                        &cfg.edge_links,
                                        cfg.edge_overshoot_px,
                                        false,
                                    ));
                                    for (id, name, _) in newly_offline {
                                        events_to_push.push(UiEvent::ComputerOffline {
                                            computer_id: id,
                                            computer_name: name,
                                        });
                                    }
                                    events_to_push.push(UiEvent::StateUpdated);
                                }
                            }

                            if keep_session {
                                let ts = now_millis_i64();
                                let ping = TouchpadPacket::Ping { timestamp: ts };
                                let pair_req = TouchpadPacket::PairRequest {
                                    client_id: cfg.client_id.clone(),
                                    client_name: shared.device_name.clone(),
                                };
                                for peer in &mut session.peers {
                                    if let Some(addr) = peer.addr {
                                        peer.awaiting_heartbeat_reply = true;
                                        packets_to_send.push((addr, ping.clone()));
                                        if peer.pairing_status != Some(PairingStatus::Rejected) {
                                            packets_to_send.push((addr, pair_req.clone()));
                                        }
                                    }
                                }
                            }
                        } else {
                            next_wakeup = next_wakeup.min(session.next_heartbeat_at);
                        }
                    }
                }

                if keep_session {
                    state.session = Some(session);
                }
            }
        }

        if let Some(new_hot) = hot_update {
            shared.set_hot_target(new_hot);
        }
        shared.send_packets(&packets_to_send);
        for ev in events_to_push {
            shared.events.push(ev);
        }

        if shared.is_shutdown() {
            break;
        }

        let wait_duration = next_wakeup.saturating_duration_since(Instant::now());
        if !wait_duration.is_zero() {
            let wake_guard = shared
                .worker_wake_lock
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !shared.is_shutdown() {
                let _ = shared.worker_wake.wait_timeout(wake_guard, wait_duration);
            }
        }
    }
}

fn find_computer_id_by_addr(
    state: &RuntimeState,
    cfg: &AndroidConfig,
    src_addr: SocketAddr,
) -> Option<String> {
    if let Some(session) = state.session.as_ref() {
        if let Some(id) = session.find_peer_id_by_addr(src_addr) {
            return Some(id);
        }
    }

    if let Some((id, _)) = state
        .endpoint_cache
        .iter()
        .find(|(_, addr)| socket_addrs_match(**addr, src_addr))
    {
        return Some(id.clone());
    }

    for comp in &cfg.computers {
        if let Ok(ip) = comp.ip_address.trim().parse::<IpAddr>() {
            let port = if comp.port > 0 { comp.port } else { DEFAULT_PORT };
            if socket_addrs_match(SocketAddr::new(ip, port), src_addr) {
                return Some(comp.id.clone());
            }
        }
    }

    None
}

fn resolve_endpoint(ip_or_host: &str, port: u16) -> Option<SocketAddr> {
    let host = ip_or_host.trim();
    if host.is_empty() || port == 0 {
        return None;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(SocketAddr::new(ip, port));
    }
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs().ok()?.collect();
    addrs
        .iter()
        .copied()
        .find(|addr| addr.is_ipv4())
        .or_else(|| addrs.into_iter().next())
}

fn sanitize_device_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "AndroidTouchpad".to_string()
    } else {
        trimmed.to_string()
    }
}

fn now_millis_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
