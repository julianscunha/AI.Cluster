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
- The worker's heartbeat and health reporting run on their own task with their own
  interval, so a long unit can no longer starve them. Sharing the work loop made the
  controller reap healthy workers mid-computation. See "Liveness" below.
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
`node-b` is then `docker kill`ed, with no leave call. The grace is set to 9s on
purpose — shorter than the hold — so a healthy worker would be reaped if the
heartbeat were still sharing the work loop.

```
enqueued 2 work units of 256x256
--- killing node-b: SIGKILL, no leave, no requeue on the way out ---
w-0   node-c    6304   12005696   12012000
w-1   node-c    5637   12008363   12014000
completed=2 pending=0 failed=0
```

Controller log:

```
controller listening on 0.0.0.0:50051 (heartbeat grace 9000ms)
node node-b offline after 9000ms without heartbeat; requeued 1 in-flight units, failed 0
work w-0 finished on node-c compute=6304us transfer=12005696us total=12012000us checksum=696.515865
work w-1 finished on node-c compute=5637us transfer=12008363us total=12014000us checksum=1354.312283
```

`requeued 1 in-flight units` is the line that matters: `node-b` was holding a unit
when it died, the unit went back to the queue, and `node-c` finished it. One offline
event for the node that was actually killed, and none for the node that was alive —
the false reap is gone. The graph drained with no failures. `node-b` never came back;
the cluster recovered without it.

The retry cap and cascade are covered by unit tests
`a_unit_is_failed_once_its_retry_budget_is_spent`,
`failing_a_unit_fails_its_dependents_with_it`, and
`a_failed_branch_does_not_fail_unrelated_work`.

## Liveness — heartbeat is decoupled from the work loop

The heartbeat and health reporting used to share the worker's sequential loop with
the work itself. A unit that ran longer than the heartbeat grace therefore starved
the heartbeat, and the controller reaped a worker that was alive and computing. The
unit was requeued, the original result was refused on arrival, and the work was done
twice — correct answer, wasted compute, one retry burned.

The heartbeat now runs on its own task on its own interval. The task is stopped
explicitly before `LeaveNode`, because a beat that landed after the departure would
make the controller re-register the node and undo it.

This is visible in the failure test, which runs a grace *shorter* than the work hold
on purpose. Before the fix, the same run logged a healthy worker being reaped over
and over until a unit ran out of retries:

```
node node-b offline after 9000ms without heartbeat; requeued 1 in-flight units, failed 0
node node-c offline after 9000ms without heartbeat; requeued 1 in-flight units, failed 0
node node-c offline after 9000ms without heartbeat; requeued 1 in-flight units, failed 0
node node-c offline after 9000ms without heartbeat; requeued 0 in-flight units, failed 1
```

After the fix, the same configuration reaps only the node that was actually killed:

```
node node-b offline after 9000ms without heartbeat; requeued 1 in-flight units, failed 0
```

Grace stays a tunable: too short relative to the heartbeat interval and a healthy
node is reaped anyway, so keep it comfortably above the interval. The leave path was
re-checked too — `node-b left` followed by `NODES 0` twelve seconds later, with no
re-registration.

## Known ceiling

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
