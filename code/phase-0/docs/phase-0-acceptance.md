---
title: "Phase 0 Acceptance Criteria"
type: operational
status: living
tags: [operational, phase-0-acceptance]
---

# Phase 0 Acceptance Criteria

## A1 Build

- Workspace resolves reproducibly. — SATISFIED: CI builds from a clean checkout.
- Controller, Worker and CLI have independent binaries. — SATISFIED:
  `ai-cluster-controller`, `ai-cluster-worker`, `ai-cluster`.
- Protocol is generated from `.proto` definitions. — SATISFIED: `build.rs` runs
  `tonic-prost-build` against `proto/cluster.proto` with vendored `protoc`.

## A2 Protocol

- Node can register. — SATISFIED: `RegisterNode` records identity and capabilities.
- Controller acknowledges registration. — SATISFIED: response carries `accepted`
  and the assigned `cluster_id`.
- Worker sends heartbeat. — SATISFIED: five-second interval; a missed grace window
  flips the node to `OFFLINE`.
- Worker sends health report. — SATISFIED: memory-pressure score streamed each tick.

## A3 Identity

- Node identity is independent of IP address. — SATISFIED: `node_id` comes from
  `AI_CLUSTER_NODE_ID` or the hostname, never the network address.
- Device identity is separate from logical node identity. — SATISFIED: `device_id`
  reads `/etc/machine-id`, so it survives a logical rename and re-imaging.

## A4 Measurement

- Benchmark harness can measure a node without changing its cluster score manually. — SATISFIED
  by `ai-cluster benchmark`; the controller records worker-reported compute time and
  its own observed round trip, so no score is entered by hand.
- Benchmark output is versioned. — SATISFIED by the `benchmark_version=... scope=...
  size=... repetitions=... seed=...` header printed with every run.

## A5 No LLM

The phase is considered complete without any model runtime. The gate is the distributed substrate, not inference quality.
