---
title: "Modelo de dados — relações"
type: data-model
status: draft
tags: [data-model, relationships]
---

# Modelo de dados — relações

Relações centrais:

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

As relações devem carregar timestamps/versão onde placement/estado mudam ao longo do tempo.
