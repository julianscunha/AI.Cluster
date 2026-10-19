# Data Model — Relationships

Core relations:

```text
Cluster HAS_NODE Node
Node REPRESENTS Device
Node HAS_CAPABILITY Capability
Node HAS_HEALTH Health
Node CONNECTED_TO Node
Model HAS_VERSION ModelVersion
ModelVersion HAS_VARIANT ModelVariant
ModelVariant HAS_SHARD ModelShard
Request CREATES Execution
Execution USES ExecutionPlan
ExecutionPlan CONTAINS WorkUnit
WorkUnit CONSUMES Tensor
WorkUnit PRODUCES Tensor
Execution HAS_STATE StateObject
ModelShard HOSTED_ON Node
StateObject LOCATED_ON Node
Request EXECUTED_BY Node
Node EXPERIENCED Event
Trace DESCRIBES Execution
```

Relationships should carry timestamps/version where placement/state changes over time.
