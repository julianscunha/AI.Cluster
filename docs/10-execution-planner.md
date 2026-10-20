---
title: "10 — Execution Planner"
type: spec
status: draft
tags: [spec, execution-planner]
---

# 10 — Execution Planner

## Objetivo

Produzir um plano de execução que minimize custo total sob as restrições do cluster.

## Entradas

- Model Graph;
- Request profile;
- Hardware Graph;
- Network Graph;
- State locality;
- Model locality;
- health;
- trust;
- Performance Model.

## Estratégias

- pipeline parallelism;
- tensor parallelism;
- hybrid parallelism.

## Função de custo

Conceitualmente:

```text
minimize(
  latency + communication + memory_pressure + failure_risk + thermal_risk + rebalancing_cost
)
```

com pesos dependentes do perfil da requisição.

## Perfis

- latency;
- throughput;
- reliability;
- efficiency;
- balanced.

## Princípio de planejamento

Um node disponível não precisa ser utilizado. Adicionar hardware pode piorar o plano se communication overhead superar o ganho computacional.

## Replanejamento

Gerar novo plan versionado em eventos relevantes. Usar hysteresis para evitar oscilação.
