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
            health: Some(self.health.clone()),
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

    pub fn report_health(&self, node_id: &str, health: HealthReport) -> bool {
        let mut nodes = self.lock();
        match nodes.get_mut(node_id) {
            Some(record) => {
                record.health = health;
                true
            }
            None => false,
        }
    }

    pub fn list(&self) -> Vec<NodeSnapshot> {
        let nodes = self.lock();
        let mut ids: Vec<&String> = nodes.keys().collect();
        ids.sort();
        ids.into_iter()
            .map(|id| nodes[id].snapshot())
            .collect()
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
        self.nodes.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
        registry.register(identity("node-a"), CapabilityReport { cpu_threads: 8, ..Default::default() }, 1_000);

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, "READY");
        assert_eq!(listed[0].capability.as_ref().unwrap().cpu_threads, 8);

        assert!(registry.heartbeat("node-a", 2_000));
        assert_eq!(registry.list()[0].last_heartbeat_unix_ms, 2_000);

        let health = HealthReport { health_score: 0.75, ..Default::default() };
        assert!(registry.report_health("node-a", health));
        assert_eq!(registry.list()[0].health.as_ref().unwrap().health_score, 0.75);
    }

    #[test]
    fn unknown_node_is_rejected_without_registering() {
        let registry = Registry::new();
        assert!(!registry.heartbeat("ghost", 1_000));
        assert!(!registry.report_health("ghost", HealthReport::default()));
        assert!(registry.list().is_empty());
    }

    #[test]
    fn stale_node_goes_offline_and_recovers() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);

        assert!(registry.mark_stale_offline(1_500, 10_000).is_empty());
        assert_eq!(registry.list()[0].state, "READY");

        assert_eq!(registry.mark_stale_offline(11_000, 10_000), vec!["node-a".to_string()]);
        assert_eq!(registry.list()[0].state, "OFFLINE");

        assert!(registry.heartbeat("node-a", 12_000));
        assert_eq!(registry.list()[0].state, "READY");
    }

    #[test]
    fn re_registration_preserves_registration_time() {
        let registry = Registry::new();
        registry.register(identity("node-a"), CapabilityReport::default(), 1_000);
        registry.register(identity("node-a"), CapabilityReport { cpu_threads: 16, ..Default::default() }, 5_000);

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].registered_at_unix_ms, 1_000);
        assert_eq!(listed[0].last_heartbeat_unix_ms, 5_000);
        assert_eq!(listed[0].capability.as_ref().unwrap().cpu_threads, 16);
    }
}
