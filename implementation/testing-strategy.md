---
title: "Testing Strategy"
type: implementation
status: proposed
tags: [implementation, testing-strategy]
---

# Testing Strategy

## Test layers

1. Unit tests.
2. Protocol compatibility tests.
3. Integration tests.
4. Fault injection.
5. Performance benchmarks.
6. Soak tests.
7. Recovery tests.

## Mandatory scenarios

### Node join

New node joins, receives configuration and benchmark.

### Node drain

Node drains while requests are active.

### Node hard failure

Kill worker/process or power off node and verify recovery.

### Thermal degradation

Artificially reduce node capability and verify scheduler changes weight.

### Network degradation

Inject latency/bandwidth reduction and verify transport-aware replanning.

### Storage degradation

Inject elevated disk latency and verify storage policy changes.

### Prediction accuracy

Compare expected vs actual execution latency and throughput.

### Token optimization

Measure:

- tokens before;
- tokens after;
- tokens avoided;
- optimizer latency;
- end-to-end latency.

## Scaling metrics

Never claim linear scaling without measurement. Compare:

```text
N nodes
vs
throughput
vs
single-request latency
vs
communication overhead
```

## Benchmark matrix

At minimum:

- same hardware / same network;
- heterogeneous hardware / same network;
- same hardware / degraded network;
- CPU-only;
- CPU + accelerator;
- RAM-only vs SSD-assisted;
- cache enabled vs disabled;
- graph retrieval vs baseline retrieval.
