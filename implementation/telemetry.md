---
title: "Telemetry — lifecycle events"
type: implementation
status: implemented
tags: [implementation, telemetry, observability, events]
---

# Telemetry — lifecycle events

The controller already printed what happened. Now it also records it, so the
timeline is readable by a client instead of only by whoever is tailing the
container log. This is H2 of the plan's Observability phase, and it closes
`ai-cluster events`, which until now printed "reserved for the telemetry phase".

## What changed

- `Event`, `ListEventsRequest`, `ListEventsResponse` and `ListEvents` in the proto.
- `EventLog` (`code/crates/controller/src/events.rs`): a bounded ring buffer of
  the last 256 events, oldest first.
- The service records at the lifecycle points: `node_registered`, `node_left`,
  `node_offline`, `work_enqueued`, `graph_enqueued`, `work_completed`.
- The offline sweep in the controller's `main` records `node_offline` too: it is
  the one lifecycle transition that happens without a request to hang it on.
- `ai-cluster events [--limit N]` prints them oldest first. Default is the last 20;
  `--limit 0` means everything still held.

## Semantics

Every event carries controller-observed time. Node clocks are never used for
ordering — the same rule as liveness: a node may report whatever time it likes, so
the only trustworthy clock is the controller's.

The log is a **ring buffer, not a store**. Past 256 events the oldest is dropped.
It answers "what just happened", which is what an operator asks during an
incident; it does not answer "what happened last week". See the ceiling.

stdout and the event log both keep the facts, and they are not redundant: the
print lines are what the E2E scripts and `docker logs` read, the events are what a
client can query. Replacing the prints would break the existing failure tests,
which parse the controller's stdout.

## Evidence

Controller plus two workers, two units, then a graceful departure of one worker:

```
EVENTS 6
AT_UNIX_MS     KIND             DETAIL
1792464749316  node_registered  node-1
1792464756107  node_registered  node-2
1792464769235  work_enqueued    2 units 64x64x64
1792464769317  work_completed   w-0 on node-1 compute=99us transfer=1901us
1792464769318  work_completed   w-1 on node-1 compute=89us transfer=911us
1792464774608  node_left        node-2 requeued=0 failed=0
```

Six events, oldest first, one line per thing that happened. Note the two units
landed on the same node — that is the ceiling below, not a typo.

The ring buffer is covered by two unit tests in `events.rs` — the oldest is
dropped once full, and `recent` returns the newest N oldest-first — and the wiring
by `lifecycle_events_are_recorded_and_listed` in `service.rs`, which checks that a
registration shows up through the RPC and that `--limit` trims from the front.

## Known ceiling

**One worker took both units.** A worker now polls again the moment it finishes a
unit, so a fast worker can empty the queue before a slower one asks. This is the
placement ceiling showing up in practice: dispatch is first-to-poll, and there is
no cost model yet to balance across nodes. Fixing it belongs to the scheduler, not
to telemetry. A short per-worker cooldown would hide it — at the cost of the
throughput the cadence fix just bought, so it is the wrong lever.

**No metrics, no traces, no prediction error.** This is H2 only. Nothing
aggregates (H1), nothing ties a client request to the work units it produced (H3),
and nothing exposes expected-vs-actual (H5). These events are a timeline, not a
dashboard.

**In memory, lost on restart.** Same as the work queue. That is the storage phase,
and the plan's chain puts it after telemetry.
