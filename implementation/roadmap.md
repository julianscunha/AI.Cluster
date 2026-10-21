---
title: "Roadmap de Implementação"
type: implementation
status: living
tags: [implementation, roadmap]
---

# Roadmap de Implementação

Status: EM EXECUÇÃO — itens executados marcados.

Legenda: `[x]` executado e validado · `[~]` parcial · `[ ]` não iniciado.

## Fase 0 — Arquitetura e harness de benchmark

Entregar:

- [x] estrutura do repositório;
- [x] esqueleto do protocolo;
- [x] esqueleto do node agent;
- [x] harness de benchmark;
- [x] esquema de telemetria — eventos (`telemetry.md`) e métricas agregadas (`metrics.md`);
- [x] workload de matriz local.

Critério de saída: dois nós Linux trocam uma Work Unit e comparam compute vs transfer.

- [~] mecânica provada com dois containers; a comparação que importa (dispatch entre
  máquinas custa menos que o compute que economiza) exige duas máquinas físicas e ainda
  não foi rodada. Runbook em `phase-0/two-machine-validation.md`.

## Fase 1 — Cluster mínimo

Entregar:

- [x] Controller;
- [x] Worker;
- [x] registration;
- [x] heartbeat;
- [x] capabilities;
- [x] health;
- [x] CLI status.

Critério de saída: nós entram e saem com segurança.

- [x] `phase-1/join-and-leave.md`; heartbeat desacoplado do laço de trabalho
  (commit `b219ec6`).

## Fase 2 — Execução distribuída

Entregar:

- [x] Work Unit;
- [x] Execution Graph;
- [x] placement — filtro por capability (`min_cpu_threads`) e por memória (footprint da
  unidade), com a escolha feita pelo modelo de custo; locality e térmico não
  (`phase-2/placement.md`);
- [~] global/local scheduler — o global escolhe por custo, com banda de histerese
  (`AI_CLUSTER_PLACEMENT=off` volta ao primeiro-que-pergunta) e o local concede os cores,
  que o worker usa (`phase-2/placement.md`, `phase-2/local-scheduler.md`); filas de recursos
  não, reservas sim (`reservations.md`);
- [~] transport abstraction — gRPC direto, sem camada de abstração;
- [x] retries.

Critério de saída: um DAG executa em vários nós e recupera de uma perda planejada de nó.

- [x] `phase-2/dependency-graph.md`, `phase-2/fault-tolerance.md`; E2E
  `phase-2-failure.sh` (worker morto → `requeued 1 in-flight units`, grafo drena).
- [~] placement: `phase-2/placement.md` — nó fraco não pega unidade pesada. Mas o
  critério da fase seguinte (melhor que round-robin) ainda não é atendido.

## Fase 3 — AI Cluster OS

Entregar:

- [ ] immutable image;
- [ ] bootstrap;
- [ ] provisioning;
- [ ] reimage;
- [ ] recovery;
- [ ] A/B update.

Critério de saída: máquina limpa vira worker automaticamente. — não iniciado.

## Fase 4 — Storage/State

Entregar:

- [ ] storage detection;
- [ ] swap/cache policies;
- [~] state directory — `AI_CLUSTER_STATE` aponta um arquivo; não é diretório de estado;
- [~] checkpoint — snapshot da fila e dos outputs, não de unidade em execução;
- [~] recovery — reload no startup do controller;
- [ ] warm/cold tiers.

Critério de saída: perda de nó não apaga estado recuperável de execução.

- [~] `persistence.md` cobre restart do **controller**. Perda de **nó** continua sendo
  recuperada por requeue (`fault-tolerance.md`), não por checkpoint.

## Fase 5 — Graph + Edge Optimizer

Entregar:

- [ ] graph representation;
- [ ] graph retrieval;
- [ ] exact/semantic cache;
- [ ] dedup;
- [ ] context selection;
- [ ] token budgeting.

Critério de saída: benchmark mostra economia mensurável de token/latência. — não iniciado.

## Fase 6 — Model Runtime

Entregar:

- [ ] model registry;
- [ ] model shards;
- [ ] quantized variants;
- [ ] CPU inference backend;
- [ ] pipeline/tensor/hybrid planning.

Critério de saída: modelo robusto executa em vários nós CPU. — não iniciado.

## Fase 7 — Heterogeneous acceleration

Entregar:

- [ ] GPU backend;
- [ ] ARM64 backend;
- [ ] NPU plugin architecture;
- [ ] USB transport;
- [ ] mobile worker prototype.

— não iniciado.

## Fase 8 — Adaptive optimization

Entregar:

- [ ] learned performance model;
- [ ] dynamic rebalancing;
- [ ] speculative execution;
- [ ] predictive prefetch;
- [ ] capacity inference.

— não iniciado.

## Fase 9 — Production hardening

Entregar:

- [ ] security hardening;
- [ ] attestation;
- [ ] upgrade/rollback;
- [ ] audit;
- [ ] failure injection;
- [ ] long-run soak tests.

— não iniciado.

## Observabilidade (Fase H do EXECUTION-PLAN)

- [x] H2 Events — `telemetry.md`, `ai-cluster events`;
- [x] H1 Metrics — roll-up de nós, fila, tempos de compute/latência, percentis p50/p95 e os
  sinais de nó (CPU, swap, temperatura, IOPS) em `metrics.md` / `ai-cluster metrics|nodes`,
  mais o exportador Prometheus em `GET /metrics`; rede e storage score não;
- [x] H3 Trace correlation — ligar requisição do cliente às units que ela gerou (`traces.md`,
  `ai-cluster request trace <id>`);
- [x] H4 Dashboard — painel de uma tela sobre as chamadas que já existem (`dashboard.md`,
  `ai-cluster dashboard [--watch N]`); TTFT, ITL, tokens/s, KV cache e previsto vs real não,
  porque dependem de fontes que ainda não existem ou não são agregadas (previsto vs real
  existe no trace, `predictions.md`, mas o `GetMetrics` ainda não soma a previsão);
- [x] H5 Previsto vs real — o modelo de custo prevê tempo, não só ordena; o trace mostra a
  previsão ao lado da medição e o erro entre as duas (`predictions.md`);
