---
title: "Benchmark Sweep — compute vs transfer"
type: implementation
status: implemented
tags: [implementation, phase-0, benchmark, scaling]
---

# Benchmark Sweep — compute vs transfer

The first experiment of the plan (§20): at what unit size does distributing a matrix
operation beat the cost of moving it? Compute grows with the cube of the edge length;
transfer grows with its square. Somewhere between them is the size below which extra
nodes do not pay for themselves. This is the Phase 0 exit criterion — "two Linux nodes
exchange a Work Unit and compare compute vs transfer".

Harness: `code/tests/scaling-sweep.sh`. Runs the same workload at several sizes across
1, 2 and 3 workers. Reproduce with
`NODES="1 2 3" SIZES="32 64 128 256" REPS=6 bash code/tests/scaling-sweep.sh`.

## Finding 1 — the poll cadence was the bottleneck, not compute or transfer

The first run showed a fixed ~36s wall time for six units on one worker, regardless of
size. Six units at one per heartbeat interval is six times five seconds: the worker's
work loop shared the heartbeat's ticker, so it asked for work once per 5s however fast
the compute was. Compute was microseconds; the wait was seconds.

The work poll now runs on its own cadence — immediately after finishing a unit, and
every `WORK_POLL_IDLE_MS` (200ms) when it found nothing.

| units | workers | before | after |
|-------|---------|--------|-------|
| 6 | 1 | 54s | 9s |
| 6 | 2 | 36s | 8s |
| 6 | 3 | 19s | 9s |

The "before" column fell with worker count only because more workers poll more often in
aggregate. That is not parallel speedup; it is the same wait sampled from more places.
This is the trap the sweep was built to expose, and it is why the sweep runs before any
scheduler work: a throughput number measured on a starved poll loop says nothing about
placement.

After the fix the wall time is roughly constant (~8s) across node counts and sizes, so
it is now dominated by fixed overhead — container start plus the CLI's 2s report poll —
not by the cluster.

## Finding 2 — within the transport ceiling, transfer dominates compute

At size 256, six units on one worker: compute 6–15.5ms, transfer 8.6–17.6ms. Transfer
still meets or exceeds compute. At smaller sizes compute is sub-millisecond and transfer
reads at its resolution floor, so 256 is the largest size where the comparison is
meaningful.

Compute grows as the cube of the edge length and transfer as its square, so a size exists
where compute overtakes transfer. That size is above 256. A 512x512 unit carries two f64
matrices, ~4MB, which is the gRPC default message ceiling. So the crossover is just past
the largest unit the current transport can carry, and reaching it needs either a larger
message limit or a different transport. Until then, the honest reading is: at every size
this cluster can actually ship, moving the unit costs more than computing it.

## Measurement caveats

- `total_latency_us` derives from millisecond timestamps, so total and transfer have ~1ms
  resolution. Fine at size 256, useless below size 128 where compute is sub-ms. The
  `transfer=0` rows in the raw output are units that completed in the same millisecond
  they were dispatched, not units with zero transfer.
- The per-unit report is cumulative across runs, so a run's `avg_*` includes every earlier
  size. Read the per-unit rows, not the run averages.
- `transfer_time_us` is `total_latency - compute_time`, so it is every non-compute cost in
  the dispatch-to-completion window: round trip, serialization and scheduling jitter. It
  is an upper bound on network transfer, not a measurement of it.

## What this does not answer

Whether the cluster beats a single node at scale. The wall time is now overhead-bound at
six units, so the sweep cannot separate compute from scheduling cost. More units per run,
or larger units once the transport ceiling moves, would.
