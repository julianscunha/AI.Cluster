---
title: "21 — Performance Model"
type: spec
status: draft
tags: [spec, performance-model]
---

# 21 — Performance Model

## Objetivo

Prever custo e capacidade e comparar previsão com resultado observado.

## Entradas

- hardware benchmark;
- historical workload metrics;
- node load;
- network topology;
- storage performance;
- model/variant;
- precision;
- context size.

## Saídas

- latency prediction;
- throughput prediction;
- node cost;
- cluster capacity;
- confidence;
- expected variance.

## Laço de aprendizado

```text
benchmark
 -> real workload
 -> observe
 -> compare expected/actual
 -> update model
 -> improve next plan
```

## Capacidade do cluster

Manter separadamente:

- latency capacity;
- throughput capacity;
- memory capacity;
- storage capacity;
- network capacity.

Não tratar soma de RAM/CPU como se fosse um único dispositivo físico.
