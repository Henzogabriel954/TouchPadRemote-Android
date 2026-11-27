use std::collections::{HashSet, VecDeque};

use touchpad_protocol::{ScreenEdge, TouchpadPacket};
use touchpad_storage::{EdgeLink, SavedComputer};

/// Boolean active-edge mask sent to a connected desktop server via `EDGE_CONFIG`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeFlags {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl EdgeFlags {
    /// Converts these flags and `overshoot_px` into a [`TouchpadPacket::EdgeConfig`].
    pub fn to_packet(self, overshoot_px: u16) -> TouchpadPacket {
        TouchpadPacket::EdgeConfig {
            left: self.left,
            right: self.right,
            top: self.top,
            bottom: self.bottom,
            overshoot_px,
        }
    }

    /// Lists enabled screen edges in deterministic order.
    pub fn enabled_edges(self) -> Vec<ScreenEdge> {
        let mut edges = Vec::with_capacity(4);
        if self.left {
            edges.push(ScreenEdge::Left);
        }
        if self.right {
            edges.push(ScreenEdge::Right);
        }
        if self.top {
            edges.push(ScreenEdge::Top);
        }
        if self.bottom {
            edges.push(ScreenEdge::Bottom);
        }
        edges
    }
}

/// Resolves the ordered list of computer IDs participating in a session rooted at `primary_id`.
///
/// - When `multi_pc_enabled` is `false`, returns only `[primary_id]`.
/// - When `multi_pc_enabled` is `true`, performs a breadth-first traversal across `edge_links`
///   (treating links as navigable in both directions) so every directly or transitively reachable
///   saved computer is included with `primary_id` first.
pub fn resolve_session_group(
    primary_id: &str,
    computers: &[SavedComputer],
    edge_links: &[EdgeLink],
    multi_pc_enabled: bool,
) -> Vec<String> {
    let valid_ids: HashSet<&str> = computers.iter().map(|c| c.id.as_str()).collect();
    if !valid_ids.contains(primary_id) {
        return Vec::new();
    }

    if !multi_pc_enabled {
        return vec![primary_id.to_string()];
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut ordered: Vec<String> = Vec::new();

    visited.insert(primary_id.to_string());
    queue.push_back(primary_id.to_string());

    while let Some(current) = queue.pop_front() {
        ordered.push(current.clone());
        for link in edge_links {
            let neighbor = if link.from_computer_id == current {
                link.to_computer_id.as_str()
            } else if link.to_computer_id == current {
                link.from_computer_id.as_str()
            } else {
                continue;
            };

            if valid_ids.contains(neighbor) && visited.insert(neighbor.to_string()) {
                queue.push_back(neighbor.to_string());
            }
        }
    }

    ordered
}

/// Finds the destination `(target_computer_id, entry_edge)` when leaving `from_computer_id` via `edge`.
///
/// Explicit outgoing links (`from_computer_id == from && from_edge == edge`) take precedence over
/// reverse matches (`to_computer_id == from && to_edge == edge`), allowing either unidirectional
/// overrides or symmetric spatial pair definitions.
pub fn find_edge_target(
    from_computer_id: &str,
    edge: ScreenEdge,
    edge_links: &[EdgeLink],
) -> Option<(String, ScreenEdge)> {
    if let Some(link) = edge_links.iter().find(|link| {
        link.from_computer_id == from_computer_id
            && link.from_edge == edge
            && link.to_computer_id != from_computer_id
    }) {
        return Some((link.to_computer_id.clone(), link.to_edge));
    }

    edge_links
        .iter()
        .find(|link| {
            link.to_computer_id == from_computer_id
                && link.to_edge == edge
                && link.from_computer_id != from_computer_id
        })
        .map(|link| (link.from_computer_id.clone(), link.from_edge))
}

/// Computes active [`EdgeFlags`] for `computer_id` based on which adjacent peers are currently
/// online and approved, Multi-PC mode state, and edge lock state.
pub fn compute_edge_flags(
    computer_id: &str,
    edge_links: &[EdgeLink],
    online_approved_ids: &HashSet<String>,
    multi_pc_enabled: bool,
    edge_locked: bool,
) -> EdgeFlags {
    if !multi_pc_enabled || edge_locked {
        return EdgeFlags::default();
    }

    let is_neighbor_ready = |edge: ScreenEdge| -> bool {
        find_edge_target(computer_id, edge, edge_links)
            .is_some_and(|(target_id, _)| online_approved_ids.contains(&target_id))
    };

    EdgeFlags {
        left: is_neighbor_ready(ScreenEdge::Left),
        right: is_neighbor_ready(ScreenEdge::Right),
        top: is_neighbor_ready(ScreenEdge::Top),
        bottom: is_neighbor_ready(ScreenEdge::Bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(id: &str) -> SavedComputer {
        SavedComputer {
            id: id.to_string(),
            name: id.to_string(),
            ip_address: "127.0.0.1".to_string(),
            port: 45454,
            connection_method_id: "NETWORK_LAN".to_string(),
            last_os: None,
            last_privilege: None,
        }
    }

    fn link(from: &str, from_edge: ScreenEdge, to: &str, to_edge: ScreenEdge) -> EdgeLink {
        EdgeLink {
            from_computer_id: from.to_string(),
            from_edge,
            to_computer_id: to.to_string(),
            to_edge,
        }
    }

    #[test]
    fn resolves_single_and_transitive_multi_pc_groups() {
        let computers = vec![pc("a"), pc("b"), pc("c"), pc("isolated")];
        let links = vec![
            link("a", ScreenEdge::Right, "b", ScreenEdge::Left),
            link("c", ScreenEdge::Top, "b", ScreenEdge::Bottom),
        ];

        assert_eq!(
            resolve_session_group("a", &computers, &links, false),
            vec!["a".to_string()]
        );

        let group = resolve_session_group("a", &computers, &links, true);
        assert_eq!(group, vec!["a", "b", "c"]);
        assert!(!group.contains(&"isolated".to_string()));
        assert!(resolve_session_group("missing", &computers, &links, true).is_empty());
    }

    #[test]
    fn finds_forward_reverse_and_overridden_edge_targets() {
        let links = vec![
            link("a", ScreenEdge::Right, "b", ScreenEdge::Left),
            link("b", ScreenEdge::Left, "c", ScreenEdge::Right),
        ];

        assert_eq!(
            find_edge_target("a", ScreenEdge::Right, &links),
            Some(("b".to_string(), ScreenEdge::Left))
        );
        // Explicit outgoing link from B's Left overrides the reverse of A->B.
        assert_eq!(
            find_edge_target("b", ScreenEdge::Left, &links),
            Some(("c".to_string(), ScreenEdge::Right))
        );
        // Reverse match still works when no explicit outgoing link exists.
        assert_eq!(
            find_edge_target("c", ScreenEdge::Right, &links),
            Some(("b".to_string(), ScreenEdge::Left))
        );
    }

    #[test]
    fn computes_dynamic_edge_flags_with_online_and_lock_state() {
        let links = vec![
            link("a", ScreenEdge::Right, "b", ScreenEdge::Left),
            link("a", ScreenEdge::Top, "c", ScreenEdge::Bottom),
        ];
        let mut online = HashSet::from(["a".to_string(), "b".to_string()]);

        assert_eq!(
            compute_edge_flags("a", &links, &online, true, false),
            EdgeFlags {
                left: false,
                right: true,
                top: false,
                bottom: false,
            }
        );

        online.insert("c".to_string());
        assert_eq!(
            compute_edge_flags("a", &links, &online, true, false),
            EdgeFlags {
                left: false,
                right: true,
                top: true,
                bottom: false,
            }
        );

        // Edge lock or single-PC mode suppresses every active edge.
        assert_eq!(
            compute_edge_flags("a", &links, &online, true, true),
            EdgeFlags::default()
        );
        assert_eq!(
            compute_edge_flags("a", &links, &online, false, false),
            EdgeFlags::default()
        );
    }
}
