# Real.AI.Cluster

Distributed heterogeneous AI compute runtime.

## Status

Architecture v0.1 consolidated. Implementation starts with Phase A: controller/worker bootstrap, capability discovery, heartbeat, telemetry and distributed benchmarks.

## Principles

- Compute plane is independent from application/agent plane.
- Consumers access inference through API; workers are internal.
- Nodes are heterogeneous but run the same AI Cluster OS/runtime contract.
- Capacity is measured, not inferred only from hardware names.
- Hot-plug and rebalancing are first-class requirements.
- Edge optimization reduces unnecessary computation before large-model inference.
- Graphs are used for knowledge, topology, execution and observability relationships.

See `docs/` for the architecture and `implementation/` for the execution plan.
