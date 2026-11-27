use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use touchpad_protocol::{TouchpadPacket, DEFAULT_PORT};

use crate::models::DiscoveredComputer;

const RECEIVE_BUFFER_BYTES: usize = 2048;

/// Collects local IPv4 broadcast destinations (`255.255.255.255` plus per-interface subnet broadcasts).
pub fn collect_broadcast_targets() -> Vec<Ipv4Addr> {
    let mut seen: HashSet<Ipv4Addr> = HashSet::new();
    let mut targets: Vec<Ipv4Addr> = Vec::new();

    let mut push_unique = |addr: Ipv4Addr| {
        if seen.insert(addr) {
            targets.push(addr);
        }
    };

    push_unique(Ipv4Addr::BROADCAST);

    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(v4) = iface.addr {
                if v4.ip.is_loopback() || v4.ip.is_link_local() || v4.ip.is_unspecified() {
                    continue;
                }
                if let Some(bcast) = v4.broadcast {
                    push_unique(bcast);
                } else {
                    let netmask_bits = u32::from(v4.netmask);
                    if netmask_bits != 0 && netmask_bits != u32::MAX {
                        let ip_bits = u32::from(v4.ip);
                        let directed = Ipv4Addr::from(ip_bits | !netmask_bits);
                        push_unique(directed);
                    }
                }
            }
        }
    }

    targets
}

/// Parses a decoded [`TouchpadPacket`] into a [`DiscoveredComputer`] if it is a valid `DISCOVER_RESP`.
pub fn parse_discover_response(
    packet: &TouchpadPacket,
    sender_ip: Option<IpAddr>,
    fallback_port: u16,
) -> Option<DiscoveredComputer> {
    let TouchpadPacket::DiscoverResponse {
        computer_name,
        ip_address,
        port,
        method,
    } = packet
    else {
        return None;
    };

    let sender_ip_str = sender_ip
        .filter(|ip| !ip.is_unspecified())
        .map(|ip| ip.to_string());
    let packet_ip = ip_address.trim();
    let resolved_ip = sender_ip_str
        .filter(|ip| !ip.is_empty())
        .unwrap_or_else(|| packet_ip.to_string());
    if resolved_ip.is_empty() {
        return None;
    }

    let resolved_port = if *port > 0 { *port } else { fallback_port };
    if resolved_port == 0 {
        return None;
    }

    let trimmed_name = computer_name.trim();
    let display_name = if trimmed_name.is_empty() {
        resolved_ip.clone()
    } else {
        trimmed_name.to_string()
    };

    Some(DiscoveredComputer {
        name: display_name,
        ip_address: resolved_ip,
        port: resolved_port,
        connection_method_id: method.id().to_string(),
    })
}

/// Discovers active TouchPad Remote desktop servers across all local IPv4 broadcast targets.
pub fn discover_computers_on_lan(
    port: u16,
    client_name: &str,
    timeout_ms: u64,
    cancel: &AtomicBool,
) -> Vec<DiscoveredComputer> {
    let target_port = if port > 0 { port } else { DEFAULT_PORT };
    let targets: Vec<SocketAddr> = collect_broadcast_targets()
        .into_iter()
        .map(|ip| SocketAddr::V4(SocketAddrV4::new(ip, target_port)))
        .collect();
    discover_computers_with_targets(target_port, client_name, timeout_ms, cancel, &targets)
}

/// Discovers active servers by sending `DISCOVER_REQ` bursts to `broadcast_targets`.
pub fn discover_computers_with_targets(
    fallback_port: u16,
    client_name: &str,
    timeout_ms: u64,
    cancel: &AtomicBool,
    broadcast_targets: &[SocketAddr],
) -> Vec<DiscoveredComputer> {
    if cancel.load(Ordering::Acquire) || fallback_port == 0 || broadcast_targets.is_empty() {
        return Vec::new();
    }

    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return Vec::new();
    };
    let _ = socket.set_broadcast(true);

    let request_bytes = TouchpadPacket::DiscoverRequest {
        client_name: client_name.to_string(),
    }
    .to_bytes();

    send_discovery_burst(&socket, &request_bytes, broadcast_targets);

    let total_ms = timeout_ms.max(150);
    let retry_ms = (total_ms / 3).clamp(100, 350);
    let start = Instant::now();
    let deadline = start + Duration::from_millis(total_ms);
    let retry_at = start + Duration::from_millis(retry_ms);
    let mut retry_sent = false;

    let mut discovered: HashMap<(String, u16), DiscoveredComputer> = HashMap::new();
    let mut order: Vec<(String, u16)> = Vec::new();
    let mut buf = [0u8; RECEIVE_BUFFER_BYTES];

    loop {
        if cancel.load(Ordering::Acquire) {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }

        if !retry_sent && now >= retry_at {
            retry_sent = true;
            if deadline.saturating_duration_since(now) > Duration::from_millis(30) {
                send_discovery_burst(&socket, &request_bytes, broadcast_targets);
            }
        }

        let next_wakeup = if !retry_sent && retry_at < deadline {
            retry_at
        } else {
            deadline
        };
        let slice = next_wakeup
            .saturating_duration_since(now)
            .min(Duration::from_millis(60))
            .max(Duration::from_millis(1));
        if socket.set_read_timeout(Some(slice)).is_err() {
            break;
        }

        match socket.recv_from(&mut buf) {
            Ok((len, src_addr)) => {
                if let Some(packet) = TouchpadPacket::from_bytes(&buf[..len]) {
                    if let Some(comp) =
                        parse_discover_response(&packet, Some(src_addr.ip()), fallback_port)
                    {
                        let key = (comp.ip_address.to_ascii_lowercase(), comp.port);
                        if !discovered.contains_key(&key) {
                            order.push(key.clone());
                        }
                        discovered.insert(key, comp);
                    }
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(_) => break,
        }
    }

    order
        .into_iter()
        .filter_map(|key| discovered.remove(&key))
        .collect()
}

fn send_discovery_burst(socket: &UdpSocket, payload: &[u8], targets: &[SocketAddr]) {
    for target in targets {
        let _ = socket.send_to(payload, target);
    }
}
