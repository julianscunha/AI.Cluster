# Data Model — Graph Model

## Graph node types

```text
Cluster
Node
Device
ComputeDevice
Storage
NetworkInterface
Model
ModelShard
Request
Execution
ExecutionPlan
WorkUnit
Tensor
StateObject
Document
Entity
Incident
Metric
Event
Skill
Tool
```

## Edge types

```text
HOSTS
CONNECTED_TO
DEPENDS_ON
PRODUCES
CONSUMES
EXECUTES
STORES
REPLICATES
RELATED_TO
AFFECTS
SIMILAR_TO
REQUIRES
SUPPORTS
```

## Principles

- graph is query/traversal optimized;
- object stores remain authoritative for large payloads;
- graph must be reconstructible;
- topology edges carry metrics and health where appropriate;
- temporal state should be versioned.
