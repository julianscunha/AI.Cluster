# Two-machine validation runbook

The same-host benchmark proves the mechanics but not the claim the spec cares
about: that dispatching a work unit across machines costs less than the compute
it saves. On one host the workers share cores, so the second worker makes the
numbers *worse*. Only separate machines answer the real question.

## What this proves

`T_total = T_compute + T_communication + T_serialization + T_memory + T_queue`

The single-node baseline (`ai-cluster benchmark --local`) is `T_compute` with no
transport. The distributed run adds everything else. Two machines on a real
network are the first setup where the comparison means anything.

## Prerequisites

- Two Linux machines (the "nodes"), same architecture, reachable over the network.
- Rust toolchain, or the `aic-rust:1` image used in CI.
- The controller binds `0.0.0.0:50051`; the worker needs to reach that port.

## Steps

On **machine A** (controller):

```sh
cargo run -p ai-cluster-controller --release
```

On **machine B** (worker), pointing at A's address:

```sh
AI_CLUSTER_NODE_ID=node-b \
AI_CLUSTER_CONTROLLER=http://<machine-a-ip>:50051 \
cargo run -p ai-cluster-worker --release
```

Confirm B registered, from machine A:

```sh
cargo run -p ai-cluster-cli --release -- nodes
```

Run the comparison — once with only B, then repeat with a second worker on A to
see whether adding a machine beats adding cores:

```sh
cargo run -p ai-cluster-cli --release -- benchmark --size 256 --repetitions 8
```

## What to record

| run | scope | avg compute | avg transfer | total |
|---|---|---|---|---|
| baseline | `--local` on A | | n/a | |
| 1 remote worker | A controller + B worker | | | |
| 2 workers | A controller + A+B workers | | | |

The distributed run only wins when `avg compute saved by the extra machine`
exceeds `avg transfer`. If transfer stays flat while compute drops, distribution
pays. If transfer grows with compute, the network is the bottleneck.

## Known ceiling

The controller timestamps dispatch and completion in milliseconds, so transfer
time has ~1ms resolution (`work.rs`). Keep the workload in the hundreds of
milliseconds per unit, or the measurement noise dominates. Tighten to microsecond
timestamps before benchmarking sub-millisecond units.
