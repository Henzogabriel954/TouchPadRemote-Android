use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use touchpad_android_core::discovery::{discover_computers_with_targets, parse_discover_response};
use touchpad_android_core::events::EventBus;
use touchpad_android_core::{
    AndroidEngine, ComputerInput, EdgeLink, EngineTimings, MouseButton, PairingStatus, ScreenEdge,
    UiEvent,
};
use touchpad_protocol::{ConnectionMethod, HostOs, PrivilegeMode, TouchpadPacket};
use touchpad_server_core::{DryRunBackend, DryRunConfig, MonitorRect, TouchpadServer};
use touchpad_storage::{ConfigStore, DesktopConfig};

static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

struct TempTestDir {
    path: PathBuf,
}

impl TempTestDir {
    fn new(prefix: &str) -> Self {
        let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("touchpad_android_test_{prefix}_{pid}_{seq}"));
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        Self { path: dir }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn fast_timings() -> EngineTimings {
    EngineTimings {
        initial_pair_timeout: Duration::from_millis(220),
        pair_poll_interval: Duration::from_millis(70),
        heartbeat_interval: Duration::from_millis(80),
        refresh_timeout: Duration::from_millis(160),
        edge_cooldown: Duration::from_millis(120),
        socket_poll_interval: Duration::from_millis(15),
        worker_tick_interval: Duration::from_millis(15),
    }
}

fn wait_for_event<F>(engine: &AndroidEngine, timeout: Duration, mut pred: F) -> Vec<UiEvent>
where
    F: FnMut(&UiEvent) -> bool,
{
    let deadline = Instant::now() + timeout;
    let mut seen = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for expected UI event; seen={seen:?}");
        }
        let slice_ms = remaining.as_millis().min(200) as u64;
        let batch = engine.wait_events(slice_ms.max(1));
        for ev in batch {
            let matched = pred(&ev);
            seen.push(ev);
            if matched {
                return seen;
            }
        }
    }
}

fn wait_until<F>(timeout: Duration, mut pred: F)
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("condition not satisfied within {timeout:?}");
}

struct MockServerState {
    name: String,
    pair_status: Option<PairingStatus>,
    respond_to_pings: bool,
    send_host_info: bool,
    received: VecDeque<TouchpadPacket>,
    last_client: Option<SocketAddr>,
}

struct MockServer {
    addr: SocketAddr,
    shared: Arc<(Mutex<MockServerState>, AtomicBool)>,
    socket: UdpSocket,
    handle: Option<JoinHandle<()>>,
}

impl MockServer {
    fn start(name: &str, pair_status: Option<PairingStatus>) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("bind mock server");
        socket
            .set_read_timeout(Some(Duration::from_millis(15)))
            .unwrap();
        let addr = socket.local_addr().unwrap();
        let reply_socket = socket.try_clone().unwrap();

        let shared = Arc::new((
            Mutex::new(MockServerState {
                name: name.to_string(),
                pair_status,
                respond_to_pings: true,
                send_host_info: true,
                received: VecDeque::new(),
                last_client: None,
            }),
            AtomicBool::new(false),
        ));

        let shared_thread = Arc::clone(&shared);
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 2048];
            while !shared_thread.1.load(Ordering::Acquire) {
                match reply_socket.recv_from(&mut buf) {
                    Ok((len, src)) => {
                        let Some(packet) = TouchpadPacket::from_bytes(&buf[..len]) else {
                            continue;
                        };
                        let mut reply: Vec<TouchpadPacket> = Vec::new();
                        {
                            let mut st = shared_thread.0.lock().unwrap();
                            st.last_client = Some(src);
                            st.received.push_back(packet.clone());

                            match packet {
                                TouchpadPacket::Ping { timestamp } => {
                                    if st.respond_to_pings {
                                        reply.push(TouchpadPacket::Pong {
                                            timestamp,
                                            server_name: st.name.clone(),
                                        });
                                        if st.send_host_info {
                                            reply.push(TouchpadPacket::HostInfo {
                                                os: HostOs::Windows,
                                                width: 1920,
                                                height: 1080,
                                                privilege: PrivilegeMode::User,
                                            });
                                        }
                                    }
                                }
                                TouchpadPacket::PairRequest { .. } => {
                                    if let Some(status) = st.pair_status {
                                        reply.push(TouchpadPacket::PairResponse {
                                            status,
                                            server_name: st.name.clone(),
                                        });
                                        if st.send_host_info {
                                            reply.push(TouchpadPacket::HostInfo {
                                                os: HostOs::Windows,
                                                width: 1920,
                                                height: 1080,
                                                privilege: PrivilegeMode::User,
                                            });
                                        }
                                    }
                                }
                                TouchpadPacket::DiscoverRequest { .. } => {
                                    reply.push(TouchpadPacket::DiscoverResponse {
                                        computer_name: st.name.clone(),
                                        ip_address: "127.0.0.1".to_string(),
                                        port: addr.port(),
                                        method: ConnectionMethod::NetworkLan,
                                    });
                                }
                                _ => {}
                            }
                        }

                        for pkt in reply {
                            let _ = reply_socket.send_to(&pkt.to_bytes(), src);
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
                    Err(_) => break,
                }
            }
        });

        Self {
            addr,
            shared,
            socket,
            handle: Some(handle),
        }
    }

    fn set_pair_status(&self, status: Option<PairingStatus>) {
        self.shared.0.lock().unwrap().pair_status = status;
    }

    fn set_respond_to_pings(&self, respond: bool) {
        self.shared.0.lock().unwrap().respond_to_pings = respond;
    }

    fn send_to_client(&self, packet: TouchpadPacket) {
        let client = self
            .shared
            .0
            .lock()
            .unwrap()
            .last_client
            .expect("mock server has no known client address");
        self.socket
            .send_to(&packet.to_bytes(), client)
            .expect("send packet to client");
    }

    fn take_received(&self) -> Vec<TouchpadPacket> {
        self.shared
            .0
            .lock()
            .unwrap()
            .received
            .drain(..)
            .collect()
    }

    fn wait_for_packet<F>(&self, timeout: Duration, mut pred: F) -> TouchpadPacket
    where
        F: FnMut(&TouchpadPacket) -> bool,
    {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            {
                let mut st = self.shared.0.lock().unwrap();
                if let Some(pos) = st.received.iter().position(&mut pred) {
                    return st.received.remove(pos).unwrap();
                }
            }
            thread::sleep(Duration::from_millis(8));
        }
        let snapshot: Vec<_> = self
            .shared
            .0
            .lock()
            .unwrap()
            .received
            .iter()
            .cloned()
            .collect();
        panic!("timed out waiting for packet on mock server; received={snapshot:?}");
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.shared.1.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[test]
fn config_persistence_crud_and_link_pruning() {
    let temp = TempTestDir::new("config_crud");
    let engine = AndroidEngine::with_timings(
        temp.path().to_path_buf(),
        "Pixel 9".to_string(),
        fast_timings(),
    )
    .unwrap();

    // Empty IP rejected.
    assert!(engine
        .add_or_update_computer_json(r#"{"name":"Bad","ip_address":"  "}"#)
        .is_err());

    let state = engine
        .add_or_update_computer_json(
            r#"{"id":"pc-1","name":"Desk PC","ipAddress":"192.168.1.10","port":45454}"#,
        )
        .unwrap();
    assert_eq!(state.computers.len(), 1);
    assert_eq!(state.computers[0].id, "pc-1");
    assert_eq!(state.computers[0].name, "Desk PC");

    // Edit by ID preserves ID and updates name/port.
    let state = engine
        .add_or_update_computer_json(
            r#"{"id":"pc-1","name":"Desk Renamed","ip_address":"192.168.1.10","port":45500}"#,
        )
        .unwrap();
    assert_eq!(state.computers.len(), 1);
    assert_eq!(state.computers[0].id, "pc-1");
    assert_eq!(state.computers[0].name, "Desk Renamed");
    assert_eq!(state.computers[0].port, 45500);

    // Add PC-2 and link PC-1 -> PC-2.
    let state = engine
        .add_or_update_computer(ComputerInput {
            id: Some("pc-2".to_string()),
            name: "Laptop".to_string(),
            ip_address: "192.168.1.20".to_string(),
            port: Some(45454),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(state.computers.len(), 2);

    // Endpoint deduplication without ID updates PC-2 in place preserving "pc-2".
    let state = engine
        .add_or_update_computer_json(
            r#"{"name":"Laptop Pro","ip_address":"192.168.1.20","port":45454}"#,
        )
        .unwrap();
    assert_eq!(state.computers.len(), 2);
    assert_eq!(state.computers[1].id, "pc-2");
    assert_eq!(state.computers[1].name, "Laptop Pro");

    let state = engine
        .save_edge_links_json(
            r#"[{"from_computer_id":"pc-1","from_edge":"RIGHT","to_computer_id":"pc-2","to_edge":"LEFT"}]"#,
        )
        .unwrap();
    assert_eq!(state.edge_links.len(), 1);

    let state = engine
        .save_settings_json(
            r#"{"multi_pc_enabled":true,"hold_duration_ms":9999,"local_sensitivity":2.0,"edge_overshoot_px":44,"haptics_enabled":false,"default_landscape":false}"#,
        )
        .unwrap();
    assert!(state.settings.multi_pc_enabled);
    assert_eq!(state.settings.hold_duration_ms, 1500);
    assert!((state.settings.local_sensitivity - 2.0).abs() < f32::EPSILON);
    assert_eq!(state.settings.edge_overshoot_px, 44);
    assert!(!state.settings.haptics_enabled);
    assert!(!state.settings.default_landscape);

    // Removing PC-2 prunes the dangling edge link.
    let state = engine.remove_computer("pc-2").unwrap();
    assert_eq!(state.computers.len(), 1);
    assert!(state.edge_links.is_empty());

    let client_id = state.client_id.clone();
    engine.shutdown();

    // Reload from disk and verify persistence.
    let reloaded = AndroidEngine::with_timings(
        temp.path().to_path_buf(),
        "Pixel 9".to_string(),
        fast_timings(),
    )
    .unwrap();
    let reloaded_state = reloaded.get_state();
    assert_eq!(reloaded_state.client_id, client_id);
    assert_eq!(reloaded_state.computers.len(), 1);
    assert_eq!(reloaded_state.computers[0].name, "Desk Renamed");
    assert!(reloaded_state.settings.multi_pc_enabled);
    assert!((reloaded_state.settings.local_sensitivity - 2.0).abs() < f32::EPSILON);
}

#[test]
fn event_bus_condvar_wakeup_coalescing_and_shutdown() {
    let bus = Arc::new(EventBus::new());

    // Coalesces consecutive StateUpdated.
    bus.push(UiEvent::StateUpdated);
    bus.push(UiEvent::StateUpdated);
    bus.push(UiEvent::ToastMessage {
        message: "hi".to_string(),
    });
    bus.push(UiEvent::StateUpdated);

    let drained = bus.wait_events(0);
    assert_eq!(
        drained,
        vec![
            UiEvent::StateUpdated,
            UiEvent::ToastMessage {
                message: "hi".to_string()
            },
            UiEvent::StateUpdated,
        ]
    );
    assert!(bus.wait_events(10).is_empty());

    // Background thread wakes blocked waiter immediately.
    let bus_bg = Arc::clone(&bus);
    let pusher = thread::spawn(move || {
        thread::sleep(Duration::from_millis(35));
        bus_bg.push(UiEvent::ToastMessage {
            message: "woke".to_string(),
        });
    });

    let started = Instant::now();
    let events = bus.wait_events(3000);
    assert!(started.elapsed() < Duration::from_millis(1000));
    assert_eq!(
        events,
        vec![UiEvent::ToastMessage {
            message: "woke".to_string()
        }]
    );
    pusher.join().unwrap();

    // Shutdown wakes blocked waiter immediately.
    let bus_shutdown = Arc::clone(&bus);
    let stopper = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        bus_shutdown.shutdown();
    });
    let started = Instant::now();
    let after_shutdown = bus.wait_events(3000);
    assert!(started.elapsed() < Duration::from_millis(1000));
    assert!(after_shutdown.is_empty());
    stopper.join().unwrap();
}

#[test]
fn discovery_parsing_and_loopback_scan() {
    // Parse checks: sender IP wins over payload IP, port 0 falls back, empty name falls back to IP.
    let parsed = parse_discover_response(
        &TouchpadPacket::DiscoverResponse {
            computer_name: "Workstation".to_string(),
            ip_address: "10.0.0.99".to_string(),
            port: 0,
            method: ConnectionMethod::NetworkLan,
        },
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50))),
        45454,
    )
    .unwrap();
    assert_eq!(parsed.name, "Workstation");
    assert_eq!(parsed.ip_address, "192.168.1.50");
    assert_eq!(parsed.port, 45454);

    let ignored = parse_discover_response(&TouchpadPacket::Ping { timestamp: 1 }, None, 45454);
    assert!(ignored.is_none());

    // Live loopback scan against MockServer.
    let server = MockServer::start("Studio-PC", None);
    let cancel = AtomicBool::new(false);
    let found = discover_computers_with_targets(
        server.addr.port(),
        "Pixel-Test",
        300,
        &cancel,
        &[server.addr],
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Studio-PC");
    assert_eq!(found[0].ip_address, "127.0.0.1");
    assert_eq!(found[0].port, server.addr.port());
}

#[test]
fn refresh_status_marks_online_offline_and_persists_host_info() {
    let temp = TempTestDir::new("refresh_status");
    let server = MockServer::start("Online-PC", None);

    // Grab an unused port for an unreachable computer.
    let unused_port = {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.local_addr().unwrap().port()
    };

    let engine = AndroidEngine::with_timings(
        temp.path().to_path_buf(),
        "Phone".to_string(),
        fast_timings(),
    )
    .unwrap();

    engine
        .add_or_update_computer(ComputerInput {
            id: Some("pc-on".to_string()),
            name: "Online PC".to_string(),
            ip_address: "127.0.0.1".to_string(),
            port: Some(server.addr.port()),
            ..Default::default()
        })
        .unwrap();
    engine
        .add_or_update_computer(ComputerInput {
            id: Some("pc-off".to_string()),
            name: "Offline PC".to_string(),
            ip_address: "127.0.0.1".to_string(),
            port: Some(unused_port),
            ..Default::default()
        })
        .unwrap();
    let _ = engine.wait_events(10);

    engine.refresh_status();

    wait_until(Duration::from_secs(2), || {
        let st = engine.get_state();
        st.online_status.get("pc-on") == Some(&true)
            && st.online_status.get("pc-off") == Some(&false)
            && st.host_info_map.contains_key("pc-on")
    });

    let state = engine.get_state();
    let pc_on = state.computers.iter().find(|c| c.id == "pc-on").unwrap();
    assert_eq!(pc_on.last_os.as_deref(), Some("WINDOWS"));
    assert_eq!(pc_on.last_privilege.as_deref(), Some("USER"));
}

#[test]
fn single_pc_pairing_rejected_offline_cancel_and_input_flow() {
    let temp = TempTestDir::new("single_pc");
    let server = MockServer::start("Single-PC", Some(PairingStatus::Rejected));

    let engine = AndroidEngine::with_timings(
        temp.path().to_path_buf(),
        "Phone".to_string(),
        fast_timings(),
    )
    .unwrap();
    engine
        .add_or_update_computer(ComputerInput {
            id: Some("pc-1".to_string()),
            name: "Single PC".to_string(),
            ip_address: "127.0.0.1".to_string(),
            port: Some(server.addr.port()),
            ..Default::default()
        })
        .unwrap();
    engine
        .save_settings_json(r#"{"local_sensitivity":1.5}"#)
        .unwrap();
    let _ = engine.wait_events(10);

    // 1. Rejected flow.
    engine.start_session("pc-1", false);
    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(ev, UiEvent::PairingRejected { computer_id, .. } if computer_id == "pc-1")
    });
    assert!(engine.get_state().active_session.is_none());

    // 2. Pending -> Cancel pairing flow.
    server.set_pair_status(Some(PairingStatus::Pending));
    engine.start_session("pc-1", false);
    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(ev, UiEvent::PairingPending { computer_id, .. } if computer_id == "pc-1")
    });
    assert_eq!(engine.get_state().pairing_computer_id.as_deref(), Some("pc-1"));
    engine.cancel_pairing();
    assert!(engine.get_state().active_session.is_none());
    assert!(engine.get_state().pairing_computer_id.is_none());

    // 3. Pending -> Approved -> Input -> Exit flow.
    let _ = engine.wait_events(10);
    engine.start_session("pc-1", false);
    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(ev, UiEvent::PairingPending { .. })
    });
    server.set_pair_status(Some(PairingStatus::Approved));
    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(ev, UiEvent::ActiveComputerChanged { computer_id, via_edge: None, .. } if computer_id == "pc-1")
    });

    let _ = server.take_received();
    engine.send_move(4.0, -2.0);
    engine.send_click(MouseButton::Right);
    engine.send_button_state(MouseButton::Left, true);
    engine.send_scroll(-3.5);

    server.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::Move { dx: 6.0, dy: -3.0 }
    });
    server.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::Click {
            button: MouseButton::Right,
        }
    });
    server.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::ButtonState {
            button: MouseButton::Left,
            pressed: true,
        }
    });
    server.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::Scroll { delta_y: -3.5 }
    });

    engine.exit_session();
    server.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::ButtonState {
            button: MouseButton::Left,
            pressed: false,
        }
    });
    server.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::ButtonState {
            button: MouseButton::Right,
            pressed: false,
        }
    });
    assert!(engine.get_state().active_session.is_none());

    // 4. Offline timeout during Connecting.
    server.set_pair_status(None);
    let _ = engine.wait_events(10);
    engine.start_session("pc-1", false);
    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(ev, UiEvent::ComputerOffline { computer_id, .. } if computer_id == "pc-1")
    });
}

#[test]
fn multi_pc_mock_loopback_switching_lock_cooldown_and_disconnect() {
    let temp = TempTestDir::new("multi_pc_mock");
    let server_a = MockServer::start("PC-Alpha", Some(PairingStatus::Approved));
    let server_b = MockServer::start("PC-Beta", Some(PairingStatus::Approved));

    let engine = AndroidEngine::with_timings(
        temp.path().to_path_buf(),
        "Phone".to_string(),
        fast_timings(),
    )
    .unwrap();
    engine
        .add_or_update_computer(ComputerInput {
            id: Some("pc-a".to_string()),
            name: "PC Alpha".to_string(),
            ip_address: "127.0.0.1".to_string(),
            port: Some(server_a.addr.port()),
            ..Default::default()
        })
        .unwrap();
    engine
        .add_or_update_computer(ComputerInput {
            id: Some("pc-b".to_string()),
            name: "PC Beta".to_string(),
            ip_address: "127.0.0.1".to_string(),
            port: Some(server_b.addr.port()),
            ..Default::default()
        })
        .unwrap();
    engine
        .save_edge_links(vec![EdgeLink {
            from_computer_id: "pc-a".to_string(),
            from_edge: ScreenEdge::Right,
            to_computer_id: "pc-b".to_string(),
            to_edge: ScreenEdge::Left,
        }])
        .unwrap();
    engine
        .save_settings_json(r#"{"local_sensitivity":2.0,"edge_overshoot_px":30}"#)
        .unwrap();
    let _ = engine.wait_events(10);

    engine.start_session("pc-a", true);

    // Wait until both PCs have received their dynamic EDGE_CONFIG.
    server_a.wait_for_packet(Duration::from_secs(2), |p| {
        *p == TouchpadPacket::EdgeConfig {
            left: false,
            right: true,
            top: false,
            bottom: false,
            overshoot_px: 30,
        }
    });
    server_b.wait_for_packet(Duration::from_secs(2), |p| {
        *p == TouchpadPacket::EdgeConfig {
            left: true,
            right: false,
            top: false,
            bottom: false,
            overshoot_px: 30,
        }
    });

    // Hot-path MOVE routes ONLY to PC-A with local_sensitivity applied.
    let _ = server_a.take_received();
    let _ = server_b.take_received();
    engine.send_move(5.0, -2.5);
    server_a.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::Move { dx: 10.0, dy: -5.0 }
    });
    assert!(!server_b
        .take_received()
        .iter()
        .any(|p| matches!(p, TouchpadPacket::Move { .. })));

    // PC-A hits its Right edge -> automatic switch to PC-B.
    let _ = engine.wait_events(10);
    server_a.send_to_client(TouchpadPacket::EdgeHit {
        edge: ScreenEdge::Right,
        ratio: 0.42,
    });

    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(
            ev,
            UiEvent::ActiveComputerChanged {
                computer_id,
                via_edge: Some(ScreenEdge::Right),
                ..
            } if computer_id == "pc-b"
        )
    });
    server_a.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::ButtonState {
            button: MouseButton::Left,
            pressed: false,
        }
    });
    server_a.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::CursorLeave {
            edge: ScreenEdge::Right,
        }
    });
    server_b.wait_for_packet(Duration::from_secs(1), |p| {
        matches!(
            p,
            TouchpadPacket::CursorEnter {
                edge: ScreenEdge::Left,
                ratio
            } if (*ratio - 0.42).abs() < 0.001
        )
    });

    // Subsequent MOVE goes ONLY to PC-B.
    let _ = server_a.take_received();
    let _ = server_b.take_received();
    engine.send_move(3.0, 4.0);
    server_b.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::Move { dx: 6.0, dy: 8.0 }
    });
    assert!(!server_a
        .take_received()
        .iter()
        .any(|p| matches!(p, TouchpadPacket::Move { .. })));

    // Immediate bounce-back EDGE_HIT from PC-B within 120ms cooldown is ignored.
    server_b.send_to_client(TouchpadPacket::EdgeHit {
        edge: ScreenEdge::Left,
        ratio: 0.5,
    });
    thread::sleep(Duration::from_millis(30));
    assert_eq!(
        engine
            .get_state()
            .active_session
            .as_ref()
            .unwrap()
            .active_computer_id,
        "pc-b"
    );

    // Wait past anti-bounce cooldown -> switching back to PC-A succeeds.
    thread::sleep(Duration::from_millis(120));
    let _ = engine.wait_events(10);
    server_b.send_to_client(TouchpadPacket::EdgeHit {
        edge: ScreenEdge::Left,
        ratio: 0.75,
    });
    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(
            ev,
            UiEvent::ActiveComputerChanged {
                computer_id,
                via_edge: Some(ScreenEdge::Left),
                ..
            } if computer_id == "pc-a"
        )
    });
    server_a.wait_for_packet(Duration::from_secs(1), |p| {
        matches!(
            p,
            TouchpadPacket::CursorEnter {
                edge: ScreenEdge::Right,
                ratio
            } if (*ratio - 0.75).abs() < 0.001
        )
    });

    // Edge lock disables edges on both PCs and blocks EDGE_HIT.
    let _ = server_a.take_received();
    let _ = server_b.take_received();
    engine.set_edge_lock(true);
    let disabled = TouchpadPacket::EdgeConfig {
        left: false,
        right: false,
        top: false,
        bottom: false,
        overshoot_px: 30,
    };
    server_a.wait_for_packet(Duration::from_secs(1), |p| *p == disabled);
    server_b.wait_for_packet(Duration::from_secs(1), |p| *p == disabled);

    thread::sleep(Duration::from_millis(130));
    server_a.send_to_client(TouchpadPacket::EdgeHit {
        edge: ScreenEdge::Right,
        ratio: 0.5,
    });
    thread::sleep(Duration::from_millis(30));
    assert_eq!(
        engine
            .get_state()
            .active_session
            .as_ref()
            .unwrap()
            .active_computer_id,
        "pc-a"
    );

    // Unlock restores edges.
    engine.set_edge_lock(false);
    server_a.wait_for_packet(Duration::from_secs(1), |p| {
        *p == TouchpadPacket::EdgeConfig {
            left: false,
            right: true,
            top: false,
            bottom: false,
            overshoot_px: 30,
        }
    });

    // Manual switch to PC-B.
    assert!(engine.switch_active_computer("pc-b"));
    assert_eq!(
        engine
            .get_state()
            .active_session
            .as_ref()
            .unwrap()
            .active_computer_id,
        "pc-b"
    );

    // Simulate PC-B going completely offline (2 missed heartbeats) -> falls back to PC-A & disables edges on PC-A.
    let _ = engine.wait_events(10);
    server_b.set_pair_status(None);
    server_b.set_respond_to_pings(false);
    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(
            ev,
            UiEvent::ActiveComputerChanged { computer_id, .. } if computer_id == "pc-a"
        )
    });
    server_a.wait_for_packet(Duration::from_secs(1), |p| *p == disabled);

    // Revoking pairing on remaining PC-A disconnects the session safely.
    let _ = engine.wait_events(10);
    server_a.set_pair_status(Some(PairingStatus::Rejected));
    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(ev, UiEvent::SessionDisconnected { .. })
    });
    assert!(engine.get_state().active_session.is_none());
}

#[test]
fn multi_pc_e2e_with_two_real_touchpad_server_core_instances() {
    let temp = TempTestDir::new("real_servers_e2e");

    let backend_a = Arc::new(DryRunBackend::new(DryRunConfig {
        monitor_bounds: vec![MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }],
        initial_pos: (960, 540),
        supports_relative_move: false,
        locked_for_game: false,
        os: HostOs::Windows,
        privilege: PrivilegeMode::User,
    }));
    let backend_b = Arc::new(DryRunBackend::new(DryRunConfig {
        monitor_bounds: vec![MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }],
        initial_pos: (960, 540),
        supports_relative_move: false,
        locked_for_game: false,
        os: HostOs::Linux,
        privilege: PrivilegeMode::Uinput,
    }));

    let store_a = Arc::new(
        ConfigStore::<DesktopConfig>::new(temp.path().join("server_a.json")).unwrap(),
    );
    store_a
        .save(DesktopConfig {
            computer_name: "Real-Server-A".to_string(),
            sensitivity: 1.0,
            edge_overshoot_px: 20,
            edge_cooldown_ms: 50,
            require_pairing_approval: true,
            ..Default::default()
        })
        .unwrap();

    let store_b = Arc::new(
        ConfigStore::<DesktopConfig>::new(temp.path().join("server_b.json")).unwrap(),
    );
    store_b
        .save(DesktopConfig {
            computer_name: "Real-Server-B".to_string(),
            sensitivity: 1.0,
            edge_overshoot_px: 20,
            edge_cooldown_ms: 50,
            require_pairing_approval: true,
            ..Default::default()
        })
        .unwrap();

    let mut server_a = TouchpadServer::with_port(
        Arc::clone(&backend_a) as Arc<dyn touchpad_server_core::InputBackend>,
        store_a,
        0,
    );
    let mut server_b = TouchpadServer::with_port(
        Arc::clone(&backend_b) as Arc<dyn touchpad_server_core::InputBackend>,
        store_b,
        0,
    );
    server_a.start().unwrap();
    server_b.start().unwrap();

    let port_a = server_a.actual_port();
    let port_b = server_b.actual_port();

    let engine = AndroidEngine::with_timings(
        temp.path().join("android"),
        "Pixel-E2E".to_string(),
        fast_timings(),
    )
    .unwrap();

    engine
        .add_or_update_computer(ComputerInput {
            id: Some("real-a".to_string()),
            name: "Real Server A".to_string(),
            ip_address: "127.0.0.1".to_string(),
            port: Some(port_a),
            ..Default::default()
        })
        .unwrap();
    engine
        .add_or_update_computer(ComputerInput {
            id: Some("real-b".to_string()),
            name: "Real Server B".to_string(),
            ip_address: "127.0.0.1".to_string(),
            port: Some(port_b),
            ..Default::default()
        })
        .unwrap();
    engine
        .save_edge_links(vec![EdgeLink {
            from_computer_id: "real-a".to_string(),
            from_edge: ScreenEdge::Right,
            to_computer_id: "real-b".to_string(),
            to_edge: ScreenEdge::Left,
        }])
        .unwrap();
    engine
        .save_settings_json(r#"{"local_sensitivity":1.0,"edge_overshoot_px":20}"#)
        .unwrap();
    let _ = engine.wait_events(10);

    // Start Multi-PC session; both servers require pairing approval.
    engine.start_session("real-a", true);
    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(ev, UiEvent::PairingPending { computer_id, .. } if computer_id == "real-a")
    });

    // Approve localhost client on both servers.
    let loopback_ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    server_a.approve_pairing(loopback_ip).unwrap();
    server_b.approve_pairing(loopback_ip).unwrap();

    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(ev, UiEvent::ActiveComputerChanged { computer_id, .. } if computer_id == "real-a")
    });
    wait_until(Duration::from_secs(2), || {
        let st = engine.get_state();
        st.active_session.as_ref().is_some_and(|s| {
            s.connected
                && s.peers.iter().all(|p| p.online && p.pairing_status == Some(PairingStatus::Approved))
                && s.peers
                    .iter()
                    .find(|p| p.computer_id == "real-a")
                    .is_some_and(|p| p.active_edges == vec![ScreenEdge::Right])
        })
    });

    // Move Server-A cursor near right edge (1917 < 1918 so no overshoot yet), then overshoot past
    // edge -> Server-A sends EDGE_HIT, AndroidEngine switches active target to Server-B and sends
    // CURSOR_ENTER(LEFT) to Server-B.
    let _ = engine.wait_events(10);
    engine.send_move(957.0, 0.0);
    wait_until(Duration::from_secs(1), || {
        touchpad_server_core::InputBackend::cursor_pos(backend_a.as_ref()) == Some((1917, 540))
    });
    engine.send_move(30.0, 0.0);

    wait_for_event(&engine, Duration::from_secs(2), |ev| {
        matches!(
            ev,
            UiEvent::ActiveComputerChanged {
                computer_id,
                via_edge: Some(ScreenEdge::Right),
                ..
            } if computer_id == "real-b"
        )
    });

    // Server-B received CURSOR_ENTER(LEFT, ~0.5) and warped near left edge (x == 4).
    wait_until(Duration::from_secs(1), || {
        touchpad_server_core::InputBackend::cursor_pos(backend_b.as_ref())
            .is_some_and(|(x, _)| x == 4)
    });

    // Now MOVE routes to Server-B and updates Server-B's DryRunBackend cursor.
    let _ = backend_a.take_actions();
    let _ = backend_b.take_actions();
    engine.send_move(50.0, 15.0);
    wait_until(Duration::from_secs(1), || {
        touchpad_server_core::InputBackend::cursor_pos(backend_b.as_ref())
            .is_some_and(|(x, _)| x == 54)
    });
    assert!(backend_a.take_actions().is_empty());

    engine.exit_session();
    server_a.stop();
    server_b.stop();
}
