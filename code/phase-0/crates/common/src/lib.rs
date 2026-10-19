use std::fmt;

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

impl fmt::Display for NodeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
