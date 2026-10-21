---
title: "Fases de Implementação — Detalhado"
type: implementation
status: living
tags: [implementation, phases]
---

# Fases de Implementação — Detalhado

Legenda: `[x]` executado e validado · `[~]` parcial · `[ ]` não iniciado.

## Pacotes de trabalho da Fase 1

### Controller core
- [x] estado do cluster em memória;
- [ ] event bus — hoje é um ring buffer de eventos (`events.rs`), não um bus;
- [x] node registry;
- [x] scheduler interface — o dispatch escolhe por custo (`placement`), com
  `AI_CLUSTER_PLACEMENT=off` para voltar ao primeiro-que-pergunta;

### Worker core
- [x] heartbeat;
- [x] capability report;
- [ ] CPU benchmark — a capability reporta threads, não mede desempenho;
- [x] Work Unit runner.

### CLI
- [x] status;
- [x] nodes;
- [ ] node show;
- [x] events.

## Pacotes de trabalho da Fase 2

- [x] dependency DAG;
- [x] queue;
- [~] placement — filtro por capability; sem cost model;
- [ ] network measurements;
- [ ] transfer protocol;
- [x] retries;
- [x] trace correlation — H3, ver `traces.md`.

## Pacotes de trabalho da Fase 3

- [ ] image build;
- [ ] bootstrap;
- [ ] installer;
- [ ] bootloader;
- [ ] identity persistence;
- [ ] A/B recovery.

## Pacotes de trabalho da Fase 4

- [ ] SSD/HDD/NVMe detection;
- [ ] SMART/health where available;
- [ ] dynamic cache allocation;
- [ ] swap manager;
- [~] checkpoint manager — snapshot da fila (`persistence.md`), não de unidade em execução.

## Pacotes de trabalho da Fase 5

- [ ] graph ingestion;
- [ ] graph queries;
- [ ] retrieval evaluator;
- [ ] exact cache;
- [ ] semantic cache;
- [ ] compression evaluator.

## Pacotes de trabalho da Fase 6

- [ ] model manifest;
- [ ] shard format;
- [ ] model cache;
- [ ] CPU kernel integration;
- [ ] distributed model execution.
