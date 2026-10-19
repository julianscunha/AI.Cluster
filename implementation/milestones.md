# Milestones

## M0 — Two-node compute proof

Acceptance:

- two Linux machines;
- discoverable by Controller;
- one can send a matrix operation to another;
- telemetry recorded.

## M1 — Three-node fault-tolerant DAG

Acceptance:

- DAG with parallel branches;
- one node can drain;
- one node can fail and execution recovers.

## M2 — Automatic provisioning

Acceptance:

- clean test machine bootstraps;
- image installed automatically;
- node rejoins as READY.

## M3 — Storage intelligence

Acceptance:

- HDD/SSD/NVMe differentiated;
- policies dynamically applied;
- dashboard shows storage health.

## M4 — LLM CPU prototype

Acceptance:

- model split across multiple CPU nodes;
- expected/actual inference metrics collected;
- communication overhead visible.

## M5 — Edge optimization

Acceptance:

- token reduction benchmark;
- cache hit benchmark;
- graph retrieval benchmark;
- optimizer overhead measured.

## M6 — Heterogeneous prototype

Acceptance:

- x86 + ARM cluster;
- different capacities;
- scheduler uses workload-specific scores.

## M7 — Mobile worker

Acceptance:

- supported phone reimaged;
- connected via USB;
- contributes measurable compute without compromising cluster stability.
