# Implementation Roadmap

Status: PROPOSED

## Phase 0 — Architecture and benchmark harness

Deliver:

- repository structure;
- protocol skeleton;
- node agent skeleton;
- benchmark harness;
- telemetry schema;
- local matrix workload.

Exit criteria: two Linux nodes can exchange a Work Unit and compare compute vs transfer.

## Phase 1 — Minimal cluster

Deliver:

- Controller;
- Worker;
- registration;
- heartbeat;
- capabilities;
- health;
- CLI status.

Exit criteria: nodes join and leave safely.

## Phase 2 — Distributed execution

Deliver:

- Work Unit;
- Execution Graph;
- placement;
- global/local scheduler;
- transport abstraction;
- retries.

Exit criteria: one DAG executes across multiple nodes and recovers from one planned node loss.

## Phase 3 — AI Cluster OS

Deliver:

- immutable image;
- bootstrap;
- provisioning;
- reimage;
- recovery;
- A/B update.

Exit criteria: clean machine is converted automatically to a worker.

## Phase 4 — Storage/State

Deliver:

- storage detection;
- swap/cache policies;
- state directory;
- checkpoint;
- recovery;
- warm/cold tiers.

Exit criteria: node loss does not erase recoverable execution state.

## Phase 5 — Graph + Edge Optimizer

Deliver:

- graph representation;
- graph retrieval;
- exact/semantic cache;
- dedup;
- context selection;
- token budgeting.

Exit criteria: benchmark shows measurable token/latency savings.

## Phase 6 — Model Runtime

Deliver:

- model registry;
- model shards;
- quantized variants;
- CPU inference backend;
- pipeline/tensor/hybrid planning.

Exit criteria: robust model executes on multiple CPU nodes.

## Phase 7 — Heterogeneous acceleration

Deliver:

- GPU backend;
- ARM64 backend;
- NPU plugin architecture;
- USB transport;
- mobile worker prototype.

## Phase 8 — Adaptive optimization

Deliver:

- learned performance model;
- dynamic rebalancing;
- speculative execution;
- predictive prefetch;
- capacity inference.

## Phase 9 — Production hardening

Deliver:

- security hardening;
- attestation;
- upgrade/rollback;
- audit;
- failure injection;
- long-run soak tests.
