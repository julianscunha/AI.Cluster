---
title: "Phase 1 — Join and Leave"
type: implementation
status: implemented
tags: [implementation, phase-1, lifecycle]
---

# Phase 1 — Join and Leave

Roadmap exit criterion for Phase 1: **nodes join and leave safely**.

## What changed

- `LeaveNode` RPC in `code/proto/cluster.proto`: a node announces a planned departure.
- `Registry::begin_drain`, `Registry::remove`, `Registry::is_available` in
  `code/crates/controller/src/registry.rs`.
- `WorkStore::requeue_for_node` in `code/crates/controller/src/work.rs`: in-flight
  units return to the queue instead of being stranded.
- The dispatch record now keeps the work unit itself, so it can be handed back.
- `RequestWork` refuses nodes that are draining, offline, failed or quarantined.
- The controller's offline sweep also requeues, so a node that goes silent without
  saying goodbye loses its in-flight work to the surviving nodes.
- The worker installs a SIGTERM/SIGINT handler and calls `LeaveNode` before exiting.

## Semantics

A unit is in flight from `claim` until `complete`. Requeue removes the dispatch
record, so a late result from the departed node is refused — only the node that
claims the unit next can complete it. Nothing runs twice and nothing is lost.

## Evidence — Docker, two nodes, one planned leave

Reproduce with `bash code/tests/phase-1-leave.sh`.

Controller plus `node-b`; `node-c` joins; `node-b` leaves on SIGTERM:

| work | node | compute_us | transfer_us | total_us |
|---|---|---|---|---|
| w-0 | node-b | 2,658 | 8,342 | 11,000 |
| w-1 | node-c | 643 | 3,357 | 4,000 |
| w-2 | node-b | 587 | 2,413 | 3,000 |
| w-3 | node-c | 2,639 | 6,361 | 9,000 |
| w-4 | node-b | 670 | 2,330 | 3,000 |
| w-5 | node-c | 1,345 | 2,655 | 4,000 |

`completed=6 pending=0`. Worker log: `left cluster node_id=node-b accepted=true
requeued=0`. Controller log: `node node-b left; requeued 0 in-flight units`.
`ai-cluster nodes` after the leave lists `node-c` only.

`requeued=0` because the worker submits its unit before the next tick, so at the
moment of departure it held nothing. The requeue path itself is covered by unit
tests `requeue_for_node_returns_in_flight_units_to_the_queue` and
`leave_node_requeues_in_flight_work_and_drops_the_node`.

## Known ceiling

The worker requests one unit per heartbeat tick, so dispatch latency is bounded by
the five-second heartbeat interval. Revisit when throughput matters more than the
simplicity of a single loop.
