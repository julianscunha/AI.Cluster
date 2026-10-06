use serde::{Deserialize, Serialize};

pub mod hardware;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DeviceId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeState {
    Discovered,
    Validating,
    Ready,
    Active,
    Degraded,
    Draining,
    Offline,
    Quarantined,
}

impl NodeState {
    pub fn is_schedulable(&self) -> bool {
        matches!(self, Self::Ready | Self::Active)
    }
}
