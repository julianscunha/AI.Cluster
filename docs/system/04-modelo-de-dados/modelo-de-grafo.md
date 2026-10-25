---
title: "Modelo de dados — modelo de grafo"
type: data-model
status: draft
tags: [data-model, graph-model]
---

# Modelo de dados — modelo de grafo

## Tipos de nó do grafo

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

## Tipos de aresta

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

## Princípios

- o grafo é otimizado para consulta/travessia;
- object stores continuam sendo autoridade para payloads grandes;
- o grafo deve ser reconstruível;
- arestas de topologia carregam métricas e health quando apropriado;
- estado temporal deve ser versionado.
