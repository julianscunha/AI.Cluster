---
title: "Phase 2 — Dependency Graph"
type: implementation
status: implemented
tags: [implementation, phase-2, execution-graph]
---

# Phase 2 — Dependency Graph

First slice of the Execution Graph: units carry `depends_on`, and the controller
only dispatches a unit once everything it depends on has completed.

## What changed

- `WorkUnit.depends_on` in `code/proto/cluster.proto`: ids that must complete first.
- `EnqueueGraph` RPC: enqueues a diamond `A -> (B, C) -> D`.
- `WorkStore::claim` scans the queue for the first unit whose dependencies are
  satisfied, so a blocked unit at the head does not stall the units behind it.
- `WorkStore::blocked` reports how many queued units are still gated.
- `WorkStore` records completed ids in a `HashSet` so dependency checks are O(1).
- Requeue keeps dependencies intact: a requeued unit is still gated as before.
- CLI: `ai-cluster graph --size N` enqueues the diamond and polls until it drains.

## Semantics

A unit is dispatchable when every id in its `depends_on` appears in the completed
set. Completion is the only event that unblocks; being enqueued, claimed or
requeued does not. The completed set is append-only for the life of the process,
so a unit whose dependency completed long ago stays dispatchable.

## Evidence — Docker, two workers, diamond

Reproduce with `bash code/tests/phase-2-graph.sh`.

`node-b` and `node-c` registered; `graph --size 128` reported `enqueued=4 blocked=3`
(A ready, B/C/D gated), then drained to `pending=0`:

| work | node | compute_us | transfer_us | total_us |
|---|---|---|---|---|
| w-0 | node-c | 766 | 6,234 | 7,000 |
| w-1 | node-b | 2,832 | 7,168 | 10,000 |
| w-2 | node-c | 887 | 2,113 | 3,000 |
| w-3 | node-b | 2,135 | 6,865 | 9,000 |

`completed=4 pending=0`. The controller log shows w-0 finishing first, then the
branches, then the join — the order the graph requires. Gating itself is covered
by unit tests `blocked_units_are_not_claimed_until_dependencies_complete`,
`claim_skips_blocked_units_to_reach_a_ready_one` and
`requeue_keeps_dependencies_intact`, plus `enqueue_graph_builds_a_diamond_with_blocked_branches`.

## Known ceiling

The units carry **no data between each other**. A, B, C and D all compute the same
seeded product, so the graph proves ordering, not data flow. A real execution graph
passes B's output to D as D's input. That is the next slice.

Dispatch latency is still bounded by the five-second heartbeat tick, so a unit is
claimed up to five seconds after it becomes ready. Fine for correctness; revisit
when throughput matters.
