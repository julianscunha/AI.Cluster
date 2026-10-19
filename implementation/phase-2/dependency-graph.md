---
title: "Phase 2 — Dependency Graph"
type: implementation
status: implemented
tags: [implementation, phase-2, execution-graph]
---

# Phase 2 — Dependency Graph

The Execution Graph slice: units carry `depends_on`, and one unit's output becomes
another unit's input. The controller dispatches a unit only once its dependencies
have completed, substituting their outputs into its bound inputs.

## What changed

- `WorkUnit.depends_on` in `code/proto/cluster.proto`: ids that must complete first.
- `WorkUnit.matrix_a_from` / `matrix_b_from`: at dispatch these inputs are replaced
  by the output of the named work id. Empty means use the seeded payload as-is.
- `EnqueueGraph` RPC: enqueues a diamond `A -> (B, C) -> D` with real bindings.
- `WorkStore::claim` scans the queue for the first ready unit, so a blocked unit at
  the head does not stall the units behind it, then fills the bound inputs from the
  outputs of the dependencies.
- `WorkStore` keeps completed outputs in a `HashMap<work_id, Vec<f64>>` — the
  controller-side holding area for intermediate results. Volatile, not persisted.
- Requeue keeps dependencies and bindings intact: a requeued unit is still gated.
- CLI: `ai-cluster graph --size N` enqueues the diamond and polls until it drains.

## Semantics

A unit is dispatchable when every id in its `depends_on` has completed **and** every
bound input is available. The binding check is defensive: controller-built graphs
always bind to declared dependencies, but a unit bound to an id that never completes
stays blocked rather than dispatching with an empty payload.

The intermediate results live in the controller's memory. A worker returns its
output matrix in the result; the controller stores it and injects it into the next
unit's input when that unit is claimed. So a result crosses the network twice —
worker to controller, controller to the next worker. A worker-to-worker path would
save a hop; add it when transfer dominates.

## Evidence — Docker, two workers, diamond with data flow

Reproduce with `bash code/tests/phase-2-graph.sh`.

`node-b` and `node-c` registered; `graph --size 128` reported `enqueued=4 blocked=3`
(A ready, B/C/D gated), then drained to `pending=0`:

| work | node | compute_us | transfer_us | total_us | checksum |
|---|---|---|---|---|---|
| w-0 (A) | node-c | 804 | 9,196 | 10,000 | 476.977911 |
| w-1 (B) | node-b | 1,117 | 3,883 | 5,000 | 150.229166 |
| w-2 (C) | node-c | 1,150 | 3,850 | 5,000 | -3293.334375 |
| w-3 (D) | node-b | 915 | 3,085 | 4,000 | -218950.430782 |

The checksums **differ**, which is the proof of data flow: B and C consume A's
output, and D consumes B's and C's outputs. Before the bindings existed every unit
computed the same seeded product and all four checksums were identical.

Gating and binding are covered by unit tests
`blocked_units_are_not_claimed_until_dependencies_complete`,
`claim_skips_blocked_units_to_reach_a_ready_one`,
`requeue_keeps_dependencies_intact`,
`dependent_unit_receives_its_dependency_output_as_input`,
`a_binding_to_an_unknown_work_id_keeps_the_unit_blocked`, and
`enqueue_graph_builds_a_diamond_with_blocked_branches`.

## Known ceiling

**Failure handling lives in its own slice.** Reassignment is now bounded: a unit
whose worker dies is handed back while its retry budget lasts and failed past it,
with its dependents failed alongside. See
[Fault Tolerance](fault-tolerance.md). The ceilings that remain — a worker that
cannot heartbeat while computing, and a worker that drops an unexecutable unit
silently — are recorded there.

**Outputs are retained for the process lifetime.** Memory grows with completed work;
prune an output once its dependents are all claimed, or cap the history, when
long-running controllers start to matter.

Dispatch latency is still bounded by the five-second heartbeat tick, so a unit is
claimed up to five seconds after it becomes ready. Fine for correctness; revisit
when throughput matters.
