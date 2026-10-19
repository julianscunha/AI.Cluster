---
title: "Next Tests"
type: operational
status: living
tags: [operational, next-tests]
---

# Next Tests

Status: VERIFIED means the check runs and its output was observed; PENDING means
it has not been executed. Same-host containers prove the mechanics, not the
cross-machine performance claim.

1. Controller registration with one worker. — VERIFIED (`ai-cluster nodes`)
2. Heartbeat loss detection. — VERIFIED (state flips READY -> OFFLINE after grace)
3. Capability report ingestion. — VERIFIED (threads/RAM/health reported by the worker)
4. Two-node TCP round trip. — VERIFIED with two workers on one host; separate
   machines still PENDING
5. Matrix benchmark single-node baseline. — VERIFIED (`ai-cluster benchmark --local`)
6. Distributed matrix benchmark with two nodes. — VERIFIED on one host; real
   hardware still PENDING
7. Compare compute time, transfer time and total latency. — VERIFIED for the
   same-host case (`ai-cluster benchmark --size N --repetitions R`)

## Measured comparison — 128x128, 4 cores, single host

| scope | avg compute | avg transfer | note |
|---|---|---|---|
| single-node (`--local`) | 41,365 us | n/a | no dispatch or network |
| 1 worker | 42,429 us | 18,570 us | compute matches the baseline |
| 2 workers | 53,064 us | 11,935 us | compute inflated by sharing four cores |

Compute on one worker matches the local baseline within noise. The two-worker
figure is worse because both workers share four cores; a real multi-machine run
is required before claiming any distribution gain. These numbers are evidence
that the harness works, not evidence about cluster performance.

## Docker two-container validation

Controller and worker run as separate containers on the `aic-net` bridge network,
so dispatch crosses a real network boundary instead of a loopback socket.

```
docker network create aic-net
docker run -d --name aic-controller --network aic-net -v aic-target:/t aic-rust:1 \
  sh -c 'exec /t/release/ai-cluster-controller'
docker run -d --name aic-worker1 --network aic-net -v aic-target:/t \
  -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 -e AI_CLUSTER_NODE_ID=node-b \
  aic-rust:1 sh -c 'exec /t/release/ai-cluster-worker'
```

Observed: `node-b` registers and reaches `READY`; 6/6 work units complete with
`pending=0`; checksums match the controller's records.

### Measured comparison — 256x256, single host, two containers

| scope | avg compute | avg transfer |
|---|---|---|
| single-node (`--local`) | 6,317 us | n/a |
| 1 container worker | 7,337 us | 9,662 us |

Transfer dominates: the worker runs on the same physical cores as the client, so
there is no compute to save and every microsecond of transfer is pure cost. This
reproduces the same-host finding above and confirms the transport, serialization
and queueing paths work across a container boundary. The distribution claim still
needs separate machines.
