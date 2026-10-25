---
title: "Modelo de dados — entidades"
type: data-model
status: draft
tags: [data-model, entities]
---

# Modelo de dados — entidades

## Entidades centrais

- Cluster
- Node
- Device
- Capability
- Health
- Transport
- Model
- ModelVersion
- ModelVariant
- ModelShard
- Request
- Execution
- ExecutionPlan
- WorkUnit
- Tensor
- StateObject
- Metric
- Event
- Trace
- GraphEntity
- GraphRelation

## Regras de identidade

Usar identificadores estáveis em string/tipo-UUID, independentes de chaves primárias relacionais.

## Separação

Node != Device.

Model != ModelVariant.

Request != Execution.

ExecutionGraph != PlacementPlan.

Health != Capacity != Trust.
