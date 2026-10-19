---
title: "Phase 2 — Fault Tolerance"
type: implementation
status: implemented
tags: [implementation, phase-2, fault-tolerance, retry]
---

# Phase 2 — Fault Tolerance

Bounded reassignment: a unit whose worker dies is handed to another node, but only
so many times. Past the budget the unit is failed, and so is everything downstream
of it, so the graph always reaches an answer instead of hanging.

## What changed

- `WorkStore` counts dispatch attempts per work id. `claim` bumps the counter; the
  count survives a hand-back because it lives in the store, not in the dispatch
  record that gets dropped.
- `requeue_for_node` hands a unit back only while its budget lasts. Past
  `MAX_ATTEMPTS` (3: one first try plus two reassignments) the unit is failed
  instead of requeued.
- `fail_cascade` fails a unit's dependents with it, transitively. A dependent waits
  on its source's *output*, and a failed unit never produces one, so without the
  cascade the dependent would sit in the queue forever and `pending()` would never
  reach zero.
- `RequeueOutcome` reports both counts, so the leave path and the offline sweep can
  log `requeued N, failed M` rather than silently dropping the difference.
- `WorkReportResponse.failed` exposes the failed count; the CLI prints `failed=`.
- Worker test hook `AI_CLUSTER_WORK_DELAY_MS` holds a claimed unit before computing
  it. Unset means no delay; it exists so a failure test can kill a worker while it
  is actually holding work.

## Semantics

Attempts are counted when a unit is claimed, so the number means "times this unit
was started", not "times it came back". A unit that completes clears its counter; a
unit that exhausts it moves to the failed set and its dispatch record is dropped, so
a late result from the dead worker is refused as usual.

Failed units are neither queued nor in flight, so `pending()` excludes them. That is
the point: a client polling for completion terminates on either outcome — drained or
gave up — instead of waiting for its own deadline. `failed=0` means the graph ran to
completion; a non-zero count means some branch could not be finished.

The cascade cannot reach a unit in flight: readiness withholds a dependent until its
source has produced output, so nothing downstream of a failure is ever dispatched.

## Evidence — Docker, two workers, worker killed mid-unit

Reproduce with `bash code/tests/phase-2-failure.sh`. Two units are enqueued so both
workers hold one; each unit is held for 12s so the kill lands while it is in flight;
`node-b` is then `docker kill`ed, with no leave call.

```
enqueued 2 work units of 256x256
--- killing node-b: SIGKILL, no leave, no requeue on the way out ---
w-0   node-c    5717   12025283   12031000
w-1   node-c   18346   12010654   12029000
completed=2 pending=0 failed=0
```

Controller log:

```
controller listening on 0.0.0.0:50051 (heartbeat grace 20000ms)
work w-0 finished on node-c compute=5717us transfer=12025283us total=12031000us checksum=696.515865
node node-b offline after 20000ms without heartbeat; requeued 1 in-flight units, failed 0
work w-1 finished on node-c compute=18346us transfer=12010654us total=12029000us checksum=1354.312283
```

`requeued 1 in-flight units` is the line that matters: `node-b` was holding a unit
when it died, the unit went back to the queue, and `node-c` finished it. The graph
drained with no failures. Neither unit's checksum is repeated across nodes, and
`node-b` never came back — the cluster recovered without it.

The retry cap and cascade are covered by unit tests
`a_unit_is_failed_once_its_retry_budget_is_spent`,
`failing_a_unit_fails_its_dependents_with_it`, and
`a_failed_branch_does_not_fail_unrelated_work`.

## Known ceiling

**A worker sends no heartbeat while it is computing.** The heartbeat lives in the
same sequential loop as the work, so a unit that runs longer than the heartbeat grace
gets its healthy worker reaped as dead. The controller then requeues a unit that is
still being computed, and the original worker's result is refused on arrival. The
work is redone, so the answer stays correct, but the compute is wasted and the unit
burns a retry.

This showed up while building the test above. With a 12s hold and a 9s grace, the
controller logged `node node-c offline ... requeued 1 in-flight units, failed 0`
repeatedly for a worker that was alive the whole time, and one unit eventually
exhausted its budget and was failed:

```
node node-b offline after 9000ms without heartbeat; requeued 1 in-flight units, failed 0
node node-c offline after 9000ms without heartbeat; requeued 1 in-flight units, failed 0
node node-c offline after 9000ms without heartbeat; requeued 1 in-flight units, failed 0
node node-c offline after 9000ms without heartbeat; requeued 0 in-flight units, failed 1
```

The fix is to decouple the heartbeat from the work loop — its own task on its own
interval — so a long unit cannot starve it. Until then, keep the grace above the
longest expected unit.

**No failure signal from the worker.** A worker that cannot execute a unit drops it
silently; the controller only learns of the failure when the node's heartbeat stops.
So a live worker holding an unexecutable unit pins that unit in flight until the node
leaves. The retry budget cannot help, because nothing triggers a hand-back. Closing
this needs a failure field on the result, so "I could not run this" is distinguishable
from "I am still running this".

**Retry does not steer away from the failing node.** A reassigned unit goes to
whichever node claims next. With one surviving node there is nowhere else to go
anyway; revisit with placement, where a unit that failed on a node should not be
handed straight back to it.
