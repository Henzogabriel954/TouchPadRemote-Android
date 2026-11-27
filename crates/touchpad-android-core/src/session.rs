use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Instant;

use touchpad_protocol::{PairingStatus, TouchpadPacket};
use touchpad_storage::EdgeLink;

use crate::graph::{compute_edge_flags, EdgeFlags};
use crate::models::{ActiveSessionSnapshot, SessionPeerSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    Connecting,
    PairingPending,
    Active,
}

#[derive(Debug, Clone)]
pub struct SessionPeer {
    pub computer_id: String,
    pub name: String,
    pub ip_address: String,
    pub port: u16,
    pub addr: Option<SocketAddr>,
    pub online: bool,
    pub pairing_status: Option<PairingStatus>,
    pub last_seen_at: Option<Instant>,
    pub awaiting_heartbeat_reply: bool,
    pub missed_heartbeats: u32,
    pub last_sent_edge_config: Option<(EdgeFlags, u16)>,
}

impl SessionPeer {
    pub fn is_routable(&self) -> bool {
        self.online && self.pairing_status == Some(PairingStatus::Approved) && self.addr.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct MultiPcSession {
    pub phase: SessionPhase,
    pub primary_computer_id: String,
    pub active_computer_id: String,
    pub multi_pc_enabled: bool,
    pub edge_locked: bool,
    pub peers: Vec<SessionPeer>,
    pub started_at: Instant,
    pub next_pair_poll_at: Instant,
    pub missed_pair_polls: u32,
    pub next_heartbeat_at: Instant,
    pub last_edge_transition_at: Option<Instant>,
}

impl MultiPcSession {
    pub fn peer(&self, computer_id: &str) -> Option<&SessionPeer> {
        self.peers.iter().find(|p| p.computer_id == computer_id)
    }

    pub fn peer_mut(&mut self, computer_id: &str) -> Option<&mut SessionPeer> {
        self.peers.iter_mut().find(|p| p.computer_id == computer_id)
    }

    pub fn find_peer_id_by_addr(&self, addr: SocketAddr) -> Option<String> {
        self.peers
            .iter()
            .find(|p| p.addr.is_some_and(|a| socket_addrs_match(a, addr)))
            .map(|p| p.computer_id.clone())
    }

    pub fn active_peer(&self) -> Option<&SessionPeer> {
        self.peer(&self.active_computer_id)
    }

    pub fn online_approved_ids(&self) -> HashSet<String> {
        self.peers
            .iter()
            .filter(|p| p.is_routable())
            .map(|p| p.computer_id.clone())
            .collect()
    }

    pub fn first_other_routable_peer(&self, excluded_id: &str) -> Option<&SessionPeer> {
        self.peers
            .iter()
            .find(|p| p.computer_id != excluded_id && p.is_routable())
    }

    pub fn compute_edge_updates(
        &mut self,
        edge_links: &[EdgeLink],
        overshoot_px: u16,
        force: bool,
    ) -> Vec<(SocketAddr, TouchpadPacket)> {
        if self.phase != SessionPhase::Active {
            return Vec::new();
        }

        let online_approved = self.online_approved_ids();
        let multi_pc_enabled = self.multi_pc_enabled;
        let edge_locked = self.edge_locked;
        let mut packets = Vec::new();

        for peer in &mut self.peers {
            if !peer.is_routable() {
                peer.last_sent_edge_config = None;
                continue;
            }
            let Some(addr) = peer.addr else {
                peer.last_sent_edge_config = None;
                continue;
            };
            let flags = compute_edge_flags(
                &peer.computer_id,
                edge_links,
                &online_approved,
                multi_pc_enabled,
                edge_locked,
            );
            let desired = (flags, overshoot_px);
            if force || peer.last_sent_edge_config != Some(desired) {
                peer.last_sent_edge_config = Some(desired);
                packets.push((addr, flags.to_packet(overshoot_px)));
            }
        }

        packets
    }

    pub fn to_snapshot(&self, edge_links: &[EdgeLink]) -> ActiveSessionSnapshot {
        let online_approved = self.online_approved_ids();
        let active_name = self
            .active_peer()
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let connected =
            self.phase == SessionPhase::Active && self.active_peer().is_some_and(SessionPeer::is_routable);

        let peers = self
            .peers
            .iter()
            .map(|peer| {
                let active_edges = if self.phase == SessionPhase::Active && peer.is_routable() {
                    compute_edge_flags(
                        &peer.computer_id,
                        edge_links,
                        &online_approved,
                        self.multi_pc_enabled,
                        self.edge_locked,
                    )
                    .enabled_edges()
                } else {
                    Vec::new()
                };

                SessionPeerSnapshot {
                    computer_id: peer.computer_id.clone(),
                    name: peer.name.clone(),
                    ip_address: peer.ip_address.clone(),
                    port: peer.port,
                    online: peer.online,
                    pairing_status: peer.pairing_status,
                    active_edges,
                }
            })
            .collect();

        ActiveSessionSnapshot {
            primary_computer_id: self.primary_computer_id.clone(),
            active_computer_id: self.active_computer_id.clone(),
            active_computer_name: active_name,
            multi_pc_enabled: self.multi_pc_enabled,
            edge_locked: self.edge_locked,
            connected,
            peers,
        }
    }
}

pub(crate) fn socket_addrs_match(a: SocketAddr, b: SocketAddr) -> bool {
    if a == b {
        return true;
    }
    if a.port() != b.port() {
        return false;
    }
    match (a.ip(), b.ip()) {
        (std::net::IpAddr::V4(v4), std::net::IpAddr::V6(v6)) => {
            v6.to_ipv4_mapped() == Some(v4)
        }
        (std::net::IpAddr::V6(v6), std::net::IpAddr::V4(v4)) => {
            v6.to_ipv4_mapped() == Some(v4)
        }
        _ => false,
    }
}
