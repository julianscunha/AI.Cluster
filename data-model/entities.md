---
title: "Data Model — Entities"
type: data-model
status: draft
tags: [data-model, entities]
---

# Data Model — Entities

## Core entities

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

## Identity rules

Use stable string/UUID-like identifiers independent of relational primary keys.

## Separation

Node != Device.

Model != ModelVariant.

Request != Execution.

ExecutionGraph != PlacementPlan.

Health != Capacity != Trust.
