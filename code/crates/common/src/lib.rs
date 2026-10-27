use std::fmt;

pub mod brand;
pub mod matrix;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeState {
    Discovered,
    Provisioning,
    Validating,
    Benchmarking,
    Ready,
    Active,
    Degraded,
    Draining,
    Quarantined,
    Failed,
    Offline,
}

impl NodeState {
    /// Stable wire representation. These values cross the protocol boundary and
    /// are part of the contract, so do not rename them casually.
    pub fn as_str(self) -> &'static str {
        match self {
            NodeState::Discovered => "DISCOVERED",
            NodeState::Provisioning => "PROVISIONING",
            NodeState::Validating => "VALIDATING",
            NodeState::Benchmarking => "BENCHMARKING",
            NodeState::Ready => "READY",
            NodeState::Active => "ACTIVE",
            NodeState::Degraded => "DEGRADED",
            NodeState::Draining => "DRAINING",
            NodeState::Quarantined => "QUARANTINED",
            NodeState::Failed => "FAILED",
            NodeState::Offline => "OFFLINE",
        }
    }
}

impl fmt::Display for NodeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_state_strings_are_stable() {
        assert_eq!(NodeState::Ready.as_str(), "READY");
        assert_eq!(NodeState::Offline.as_str(), "OFFLINE");
        assert_eq!(NodeState::Ready.to_string(), "READY");
    }
}
