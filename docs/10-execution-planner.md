# 10 — Execution Planner

## Objective

Produzir um plano de execução que minimize custo total sob as restrições do cluster.

## Inputs

- Model Graph;
- Request profile;
- Hardware Graph;
- Network Graph;
- State locality;
- Model locality;
- health;
- trust;
- Performance Model.

## Strategies

- pipeline parallelism;
- tensor parallelism;
- hybrid parallelism.

## Cost function

Conceitualmente:

```text
minimize(
  latency + communication + memory_pressure + failure_risk + thermal_risk + rebalancing_cost
)
```

com pesos dependentes do perfil da requisição.

## Profiles

- latency;
- throughput;
- reliability;
- efficiency;
- balanced.

## Planning principle

Um node disponível não precisa ser utilizado. Adicionar hardware pode piorar o plano se communication overhead superar o ganho computacional.

## Replanning

Gerar novo plan versionado em eventos relevantes. Usar hysteresis para evitar oscilação.
