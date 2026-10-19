---
title: "Repository Structure"
type: operational
status: living
tags: [operational, repository-structure]
---

# Repository Structure

## Purpose

The implementation is a Rust workspace with explicit control-plane and data-plane boundaries.

```text
ai-cluster/
├── crates/
│   ├── controller/
│   ├── worker/
│   ├── common/
│   ├── protocol/
│   ├── transport/
│   ├── benchmark/
│   └── cli/
├── proto/
├── os/
├── docs/
├── config/
└── tests/
```

## Rules

- `common` contains shared domain types only.
- `protocol` contains wire contracts and generated RPC types.
- `controller` owns cluster state and scheduling decisions.
- `worker` owns local execution and hardware interaction.
- `transport` abstracts physical communication.
- `benchmark` measures capability; it does not decide placement.
- `cli` is an operator interface and never executes worker logic directly.
- No application/agent code belongs inside the compute plane.
