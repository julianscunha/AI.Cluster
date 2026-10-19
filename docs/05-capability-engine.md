# 05 — Capability Engine

## Objective

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

## Dynamic capability

A capacidade efetiva é ajustada conforme:

- thermal throttling;
- memory pressure;
- current load;
- network degradation;
- storage degradation;
- power/energy policy.

## Scores

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

## Model

`estimated_cost(work, node)` deve ser uma função do workload e do estado atual do node.

## Confidence

Toda estimativa deve carregar uma medida de confiança derivada da quantidade/qualidade dos dados históricos.
