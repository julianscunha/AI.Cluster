---
title: "Phase 2 — Placement"
type: implementation
status: implemented
tags: [implementation, phase-2, placement, scheduler, capability]
---

# Phase 2 — Placement

Candidate filtering: a work unit declares the capability it needs, and the
controller refuses to hand it to a node that does not have it. Dispatch stops
being first-come-first-served and becomes a decision.

This is the first piece of the scheduler. It filters candidates; it does not yet
cost them. Choosing *among* several capable nodes needs the cost model and comes
later.

## What changed

- `WorkUnit.min_cpu_threads` (proto field 10) declares the requirement. `0` means
  no requirement, which is every unit the controller builds today except the ones
  a client marks.
- `EnqueueWorkRequest.min_cpu_threads` (proto field 6) lets a client set it.
- `WorkStore::claim` takes the asking node's `cpu_threads` and skips units the
  node cannot run. The node keeps polling and gets the next unit it may take.
- `Registry::capability` exposes the last reported `CapabilityReport` for one
  node. The registry already stored it; nothing read it back per node before.
- `request_work` reads the node's capability and passes its `cpu_threads` to
  `claim`, so the filter runs at the only place work is handed out.
- The worker reports fewer threads than the host has when
  `AI_CLUSTER_CPU_THREADS` is set. Unset means report reality. It exists so a
  single host can stand in for two machines of different sizes.
- `ai-cluster benchmark --min-threads N` sets the requirement on the units it
  enqueues.

## Semantics

The requirement is **hard**, not a preference. A unit that needs more threads than
a node has is not handed to it at all — even if that means the unit waits. A node
that reports fewer threads than a unit needs gets the next unit it *can* run, or
nothing.

This matters because the dispatch model is pull-based: workers poll
`RequestWork`, and the controller's only lever is what to give each asker. So
placement is expressed as a filter on the queue, not as a controller-side choice
of node. The first ready unit that both has its dependencies met and fits the
asker is the one that goes out.

Two consequences follow, and both are deliberate:

- A unit whose requirement no surviving node meets stays pending. This is the
  same "cluster cannot run this" case as an empty cluster; there is no separate
  handling and no error returned. See the ceiling below.
- The filter is applied per poll. A weak node polling in a tight loop does not
  take a heavy unit; it just gets no work until a light unit appears or a capable
  node takes the heavy one.

## Evidence

Reproduce with `bash code/tests/phase-2-placement.sh`. A weak node reporting 2
threads comes up alone; a unit requiring 4 threads is enqueued; nothing finishes;
then a strong node reporting 8 threads joins and takes the unit.

```
--- nodes: the weak node reports 2 threads ---
NODES 1
NODE                 STATE      ARCH       THREADS    RAM_GB   HEALTH
node-weak            READY      x86_64           2      3.77     0.73
=== enqueue a unit needing 4 threads; only the weak node is present ===
enqueued 1 work units of 256x256
--- controller log during the weak-only window ---
OK: nothing finished; the weak node did not take the heavy unit
=== start the strong node; the waiting unit should go to it ===
WORK       NODE             COMPUTE_US  TRANSFER_US     TOTAL_US
w-0        node-strong            5708         8292        14000
completed=1 pending=0 failed=0 avg_compute_us=5708 avg_transfer_us=8292
```

`w-0 node-strong` is the line that matters. The weak node was the only node
present while the unit was queued and it never ran it; the unit finished on the
strong node as soon as it joined. First-come-first-served would have handed the
unit to the weak node the moment it polled, and the run would have finished with
`w-0 node-weak`.

The filter itself is covered by the unit test
`a_weak_node_is_skipped_for_a_unit_that_requires_more_threads`, which checks the
three cases in one place: a weak node gets the light unit instead of the heavy one,
a capable node gets the heavy one, and a weak node alone gets nothing.

## Known ceiling

**Only cpu_threads is filtered.** Memory, precision, accelerators and storage are
reported by the worker and stored by the registry, but no unit can require them
yet. The requirement field is one number; extend it to a requirements message when
the next capability matters.

**A unit whose requirement no node meets stays pending.** No error, no fail-fast:
the client polls until its own deadline. Placement cannot detect "this will never
run" because it only sees the node that is asking, not the whole cluster. Closing
this means resolving requirements against the registry when the unit is enqueued,
or when the last capable node leaves.

**No cost model.** Among nodes that all satisfy the requirement, the first to poll
wins. Choosing the cheapest capable node needs compute, communication and queue
estimates — the next scheduler slice.
