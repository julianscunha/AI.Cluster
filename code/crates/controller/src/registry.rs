use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use ai_cluster_common::NodeState;
use ai_cluster_protocol::cluster::{CapabilityReport, HealthReport, NodeIdentity, NodeSnapshot};

/// Controller-observed clock. Node-supplied timestamps are never used for liveness.
pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether the controller may hand new work to a node in this state. A node that
/// is leaving or believed dead is not a candidate, so it never wins a unit.
fn eligible(state: NodeState) -> bool {
    !matches!(
        state,
        NodeState::Draining | NodeState::Quarantined | NodeState::Failed | NodeState::Offline
    )
}

#[derive(Clone, Debug)]
pub struct NodeRecord {
    pub identity: NodeIdentity,
    pub capability: CapabilityReport,
    pub health: HealthReport,
    pub state: NodeState,
    pub registered_at_unix_ms: u64,
    pub last_heartbeat_unix_ms: u64,
}

impl NodeRecord {
    pub fn snapshot(&self) -> NodeSnapshot {
        NodeSnapshot {
            identity: Some(self.identity.clone()),
            capability: Some(self.capability.clone()),
            health: Some(self.health),
            state: self.state.as_str().to_string(),
            registered_at_unix_ms: self.registered_at_unix_ms,
            last_heartbeat_unix_ms: self.last_heartbeat_unix_ms,
        }
    }
}

#[derive(Debug, Default)]
pub struct Registry {
    nodes: Mutex<HashMap<String, NodeRecord>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Upserts a node. Re-registration refreshes capability and liveness but
    /// preserves the original registration time.
    pub fn register(
        &self,
        identity: NodeIdentity,
        capability: CapabilityReport,
        now: u64,
    ) -> NodeRecord {
        let mut nodes = self.lock();
        nodes
            .entry(identity.node_id.clone())
            .and_modify(|record| {
                record.identity = identity.clone();
                record.capability = capability.clone();
                record.state = NodeState::Ready;
                record.last_heartbeat_unix_ms = now;
            })
            .or_insert_with(|| NodeRecord {
                identity: identity.clone(),
                capability: capability.clone(),
                health: HealthReport::default(),
                state: NodeState::Ready,
                registered_at_unix_ms: now,
                last_heartbeat_unix_ms: now,
            })
            .clone()
    }

    /// Returns false when the node is unknown, so the caller can ask it to re-register.
    pub fn heartbeat(&self, node_id: &str, now: u64) -> bool {
        let mut nodes = self.lock();
        match nodes.get_mut(node_id) {
            Some(record) => {
                record.last_heartbeat_unix_ms = now;
                if record.state == NodeState::Offline {
                    record.state = NodeState::Ready;
                }
                true
            }
            None => false,
        }
    }

    /// Records a health report. An optional capability profile travels beside it: when the
    /// node remeasured its compute while idle it sends a fresh profile, and the registry
    /// takes the new numbers while keeping the registration identity and timestamp (Fase C3).
    /// A capability of `None` means the node only reported health.
    pub fn report_health(
        &self,
        node_id: &str,
        health: HealthReport,
        capability: Option<CapabilityReport>,
    ) -> bool {
        let mut nodes = self.lock();
        match nodes.get_mut(node_id) {
            Some(record) => {
                record.health = health;
                if let Some(capability) = capability {
                    record.capability = capability;
                }
                true
            }
            None => false,
        }
    }

    /// Whether the node is registered at all, regardless of its state.
    pub fn knows(&self, node_id: &str) -> bool {
        self.lock().contains_key(node_id)
    }

    /// Capability as last reported by the node. None when the node is unknown.
    pub fn capability(&self, node_id: &str) -> Option<CapabilityReport> {
        self.lock()
            .get(node_id)
            .map(|record| record.capability.clone())
    }

    /// Whether the controller may hand new work to this node. Unknown nodes and
    /// nodes leaving or believed dead are not eligible.
    pub fn is_available(&self, node_id: &str) -> bool {
        match self.lock().get(node_id) {
            Some(record) => eligible(record.state),
            None => false,
        }
    }

    /// Marks a known node as draining so no further work is dispatched to it.
    /// Returns false when the node is unknown, which means it already left.
    pub fn begin_drain(&self, node_id: &str) -> bool {
        let mut nodes = self.lock();
        match nodes.get_mut(node_id) {
            Some(record) => {
                record.state = NodeState::Draining;
                true
            }
            None => false,
        }
    }

    /// Removes a node from the registry. Returns false when it was unknown.
    pub fn remove(&self, node_id: &str) -> bool {
        self.lock().remove(node_id).is_some()
    }

    pub fn list(&self) -> Vec<NodeSnapshot> {
        let nodes = self.lock();
        let mut ids: Vec<&String> = nodes.keys().collect();
        ids.sort();
        ids.into_iter().map(|id| nodes[id].snapshot()).collect()
    }

    /// Available nodes as `(node_id, capability, health)`, sorted by id. This is the
    /// candidate set placement ranks over, read in one lock so a node cannot change
    /// state halfway through the ranking.
    pub fn candidates(&self) -> Vec<(String, CapabilityReport, HealthReport)> {
        let nodes = self.lock();
        let mut ids: Vec<&String> = nodes.keys().collect();
        ids.sort();
        ids.into_iter()
            .filter(|id| eligible(nodes[*id].state))
            .map(|id| {
                let record = &nodes[id];
                (id.clone(), record.capability.clone(), record.health)
            })
            .collect()
    }

    /// Registered node count and how many of them are available for work, read in one
    /// lock so the two numbers cannot come from different states of the world.
    pub fn counts(&self) -> (u32, u32) {
        let nodes = self.lock();
        let total = nodes.len() as u32;
        let available = nodes
            .values()
            .filter(|record| eligible(record.state))
            .count() as u32;
        (total, available)
    }

    /// Marks nodes that missed `grace_ms` of controller-observed heartbeats as
    /// offline and returns the ids that transitioned.
    pub fn mark_stale_offline(&self, now: u64, grace_ms: u64) -> Vec<String> {
        let mut nodes = self.lock();
        let mut transitioned = Vec::new();
        for record in nodes.values_mut() {
            if record.state == NodeState::Offline {
                continue;
            }
            if now.saturating_sub(record.last_heartbeat_unix_ms) >= grace_ms {
                record.state = NodeState::Offline;
                transitioned.push(record.identity.node_id.clone());
            }
        }
        transitioned.sort();
        transitioned
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, NodeRecord>> {
        self.nodes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(node_id: &str) -> NodeIdentity {
        NodeIdentity {
            node_id: node_id.to_string(),
            device_id: format!("{node_id}-device"),
            architecture: "x86_64".to_string(),
            os_version: "linux".to_string(),
            runtime_version: "0.1.0".to_string(),
        }
    }

    #[test]
    fn register_heartbeat_and_health_round_trip() {
        let registry = Registry::new();
        registry.register(
            identity("node-a"),
            CapabilityReport {
                cpu_threads: 8,
                ..Default::default()
            },
            1_000,
        );

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, "READY");
        assert_eq!(listed[0].capability.as_ref().unwrap().cpu_threads, 8);

        assert!(registry.heartbeat("node-a", 2_000));
        assert_eq!(registry.list()[0].last_heartbeat_unix_ms, 2_000);

        let health = HealthReport {
            health_score: 0.75,
            ..Default::default()
        };
        assert!(registry.report_health("node-a", health, None));
        assert_eq!(
            registry.list()[0].health.as_ref().unwrap().health_score,
            0.75
        );
    }

    #[test]
    fn unknown_node_is_rejected_without_registering() {
        let registry = Registry::new();
        assert!(!registry.heartbeat("ghost", 1_000));
        assert!(!registry.report_health("ghost", HealthReport::default(), None));
        assert!(registry.list().is_empty());
    }

    #[test]
    fn a_health_report_can_refresh_the_capability() {
        let registry = Registry::new();
        registry.register(
            identity("node-a"),
            CapabilityReport {
                cpu_threads: 4,
                compute_score: 2.0,
                ..Default::default()
            },
            1_000,
        );

        // Health only: the registered profile survives untouched.
        assert!(registry.report_health("node-a", HealthReport::default(), None));
        assert_eq!(
            registry.list()[0]
                .capability
                .as_ref()
                .unwrap()
                .compute_score,
            2.0
        );

        // With a profile: the current numbers land in the registry while the baseline and
        // the registration identity stay put (Fase C3).
        let refreshed = CapabilityReport {
            cpu_threads: 4,
            compute_score: 2.0,
            current_compute_score: 1.5,
            compute_confidence: 0.8,
            ..Default::default()
        };
        assert!(registry.report_health("node-a", HealthReport::default(), Some(refreshed)));
        let stored = registry.list()[0].capability.clone().unwrap();
        assert_eq!(stored.compute_score, 2.0);
        assert_eq!(stored.current_compute_score, 1.5);
        assert_eq!(stored.compute_confidence, 0.8);
        // The refresh must not look like a re-registration.
        assert_eq!(registry.list()[0].registered_at_unix_ms, 1_000);
    }

    #[test]
    fn stale_node_goes_offline_and_recovers() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);

        assert!(registry.mark_stale_offline(1_500, 10_000).is_empty());
        assert_eq!(registry.list()[0].state, "READY");

        assert_eq!(
            registry.mark_stale_offline(11_000, 10_000),
            vec!["node-a".to_string()]
        );
        assert_eq!(registry.list()[0].state, "OFFLINE");

        assert!(registry.heartbeat("node-a", 12_000));
        assert_eq!(registry.list()[0].state, "READY");
    }

    #[test]
    fn re_registration_preserves_registration_time() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        registry.register(
            identity("node-a"),
            CapabilityReport {
                cpu_threads: 16,
                ..Default::default()
            },
            5_000,
        );

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].registered_at_unix_ms, 1_000);
        assert_eq!(listed[0].last_heartbeat_unix_ms, 5_000);
        assert_eq!(listed[0].capability.as_ref().unwrap().cpu_threads, 16);
    }

    #[test]
    fn draining_node_stops_taking_work_and_can_be_removed() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        assert!(registry.is_available("node-a"));

        assert!(registry.begin_drain("node-a"));
        assert!(!registry.is_available("node-a"));
        assert_eq!(registry.list()[0].state, "DRAINING");

        // Draining must survive a heartbeat; the node is leaving, not recovering.
        assert!(registry.heartbeat("node-a", 2_000));
        assert_eq!(registry.list()[0].state, "DRAINING");

        assert!(registry.remove("node-a"));
        assert!(!registry.knows("node-a"));
        assert!(registry.list().is_empty());

        // Leaving twice is not an error.
        assert!(!registry.begin_drain("node-a"));
        assert!(!registry.remove("node-a"));
    }

    #[test]
    fn offline_and_unknown_nodes_are_not_available_for_work() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        assert!(registry.is_available("node-a"));

        registry.mark_stale_offline(11_000, 10_000);
        assert!(!registry.is_available("node-a"));
        assert!(!registry.is_available("ghost"));
    }
}
