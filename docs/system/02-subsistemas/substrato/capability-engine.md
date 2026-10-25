---
title: "05 — Capability Engine"
type: spec
status: draft
tags: [spec, capability-engine]
---

# 05 — Motor de capacidade

## Objetivo

Medir capacidade real do hardware e converter resultados em um perfil utilizável pelo planner.

## Baseline

O benchmark inicial mede:

- general compute;
- matmul;
- memory bandwidth;
- storage throughput/latency;
- network latency/bandwidth;
- accelerator kernels disponíveis;
- workload-specific kernels.

## Capacidade dinâmica

A capacidade efetiva é ajustada conforme:

- thermal throttling;
- memory pressure;
- current load;
- network degradation;
- storage degradation;
- power/energy policy.

## Pontuações

Não usar apenas um score global.

```text
CapabilityProfile
  general_compute
  matmul_int4
  matmul_int8
  fp16
  memory
  memory_bandwidth
  gpu
  npu
  network
  storage
  thermal_headroom
```

## Modelo

`estimated_cost(work, node)` deve ser uma função do workload e do estado atual do node.

## Confiança

Toda estimativa deve carregar uma medida de confiança derivada da quantidade/qualidade dos dados históricos.
