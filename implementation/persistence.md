---
title: "Controller state persistence"
type: implementation
status: implemented
tags: [implementation, persistence, storage, state]
---

# Controller state persistence

The controller kept its queue in memory. A restart — a crash, a redeploy, a killed
container — lost everything queued and every unit that had not completed. This is the
first piece of the plan's storage phase: the work queue and its outputs survive a
restart. See `work.rs:68` for where the previous slice recorded the gap.

## What changed

- `WorkSnapshot` and `WorkOutput` in the proto: the on-disk shape of the store.
- `WorkStore::with_state(path)`: opens a store backed by the snapshot at `path`,
  loading it when one exists. A missing file is a first run, not an error.
- `WorkStore::save_locked`: writes the whole store after each mutation that changes
  what is queued or completed.
- `AI_CLUSTER_STATE` names the snapshot file. Unset means memory only, which is what the
  benchmark and test runs use.

No new dependency: `prost` is already in the tree through `ai-cluster-protocol`, and the
snapshot reuses the generated message types rather than growing a second serialization
path.

## Semantics

The snapshot holds what is **not yet done**: the queue, plus whatever was in flight,
plus the outputs of what completed. On reload, in-flight units go back to the queue. The
worker that held one may still be running, but the reloaded controller has no dispatch
record for it, so that result would be refused. Re-offering the unit is what keeps the
work from vanishing. A unit may therefore run twice; for a deterministic matrix multiply
that is the same answer.

Deliberately **not** saved on `claim`. A unit in flight when the controller dies is still
in the previous snapshot's `pending`, so it returns without a save on every poll. Saving
per claim would put the whole state on disk at the poll rate — with the 200ms idle poll,
a write per worker per poll — for no added safety.

Written to a sibling file first, then renamed into place. A crash mid-write leaves the
previous snapshot intact rather than a truncated one. A failed write is reported and
never fatal: the in-memory state stays authoritative for the run. An unreadable snapshot
is treated as empty and reported, on the same reasoning — a corrupt file must not stop
the cluster from taking work.

## Evidence

A diamond DAG enqueued with no worker running, the controller killed, then a fresh
controller over the same file:

```
state loaded from /t/state.bin: 4 pending unit(s)
controller listening on 0.0.0.0:50051 (heartbeat grace 15000ms)
```

A worker then joined and the graph drained without being re-enqueued:

```
EVENTS 5
1792467794202  node_registered  node-1
1792467794204  work_completed   w-0 on node-1 compute=94us transfer=1906us
1792467794207  work_completed   w-1 on node-1 compute=88us transfer=912us
1792467794210  work_completed   w-2 on node-1 compute=88us transfer=912us
1792467794212  work_completed   w-3 on node-1 compute=88us transfer=0us
```

Four units, from a queue the first controller never got to dispatch. The regression
check is `a_snapshot_round_trips_pending_work_and_outputs` in `work.rs`: it asserts a
dependent unit is *ready* after reload, which only holds if its dependency's output came
back with it, and that the id sequence continued instead of restarting.

## Known ceiling

**The whole state is rewritten on every mutation.** Cost grows with queue size, and each
completion rewrites the outputs of all of them. Fine at benchmark scale; move to an
append-only log or a periodic snapshot when the queue gets large or mutations get
frequent. Marked with a `ponytail:` comment at the write.

**The registry is not persisted.** Nodes recover on their own — a worker whose heartbeat
is not acknowledged re-registers — but between a restart and the first heartbeat,
`ai-cluster nodes` shows nothing. That window is the grace interval.

**Events are not persisted.** The event log starts empty on reload. The work queue is
what had to survive; the timeline is a recent-incident view by design.

**Not the whole storage phase.** This is the queue and its outputs. Checkpointing a unit
mid-compute, replicas, and HOT/WARM/COLD tiers are separate pieces of the same phase.
